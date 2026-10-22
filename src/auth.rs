//! Pairing and device tokens (D-S6).
//!
//! `blotter-sync pair` opens a single-use code; a device trades it, with its
//! id and name, for a token. Codes and tokens are kept only as SHA-256
//! hashes. A token can be revoked from the CLI or by the device itself.

use base64::Engine;
use blotter_core::time::{self, Instant};
use chrono::Duration;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::api::{ApiError, ErrorCode};
use crate::store::Store;

/// How long a pairing code stays open.
pub const CODE_LIFETIME_MIN: i64 = 10;
/// Wrong guesses before a code is burned.
pub const MAX_FAILURES: i64 = 5;
/// Code letters: no 0/O, 1/I/L, so it reads aloud and types cleanly.
const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
const CODE_LEN: usize = 8;
/// Longest device name, in characters (as the app's).
pub const NAME_MAX: usize = 64;

fn random_bytes<const N: usize>() -> Result<[u8; N], ApiError> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).map_err(ApiError::internal)?;
    Ok(bytes)
}

fn hash(secret: &str) -> String {
    let digest = Sha256::digest(secret.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// A code as typed: case, spaces and dashes don't matter.
fn normalize(code: &str) -> String {
    code.chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .flat_map(char::to_uppercase)
        .collect()
}

/// A new pairing code, `ABCD-EFGH`.
fn new_code() -> Result<String, ApiError> {
    let bytes = random_bytes::<CODE_LEN>()?;
    let letters: String = bytes
        .iter()
        // 31 letters: the slight bias of `% 31` doesn't matter for a code
        // that lives ten minutes and allows five guesses.
        .map(|b| CODE_ALPHABET[usize::from(*b) % CODE_ALPHABET.len()] as char)
        .collect();
    Ok(format!("{}-{}", &letters[..4], &letters[4..]))
}

fn sql(err: rusqlite::Error) -> ApiError {
    ApiError::internal(err)
}

/// An open pairing code, as `blotter-sync pair` prints it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenCode {
    pub code: String,
    pub expires_at: Instant,
}

/// Opens a new pairing code, replacing any still open.
pub fn open_pairing(store: &Store, now: Instant) -> Result<OpenCode, ApiError> {
    let code = new_code()?;
    let expires_at = time::truncate(now) + Duration::minutes(CODE_LIFETIME_MIN);
    store
        .conn()
        .execute(
            "INSERT INTO pairing (id, code_hash, expires_at, failures) VALUES (1, ?1, ?2, 0) \
             ON CONFLICT (id) DO UPDATE SET code_hash = excluded.code_hash, \
             expires_at = excluded.expires_at, failures = 0",
            (hash(&normalize(&code)), time::fmt_instant(expires_at)),
        )
        .map_err(sql)?;
    Ok(OpenCode { code, expires_at })
}

/// Whether a pairing code is waiting to be used.
pub fn pairing_open(store: &Store, now: Instant) -> Result<bool, ApiError> {
    let expires: Option<String> = store
        .conn()
        .query_row("SELECT expires_at FROM pairing WHERE id = 1", [], |r| {
            r.get(0)
        })
        .optional()
        .map_err(sql)?;
    Ok(expires.is_some_and(|e| e > time::fmt_instant(now)))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairRequest {
    pub code: String,
    /// The device's id from Blotter (a ULID), kept for good.
    pub device_id: String,
    /// What the user calls it ("Desktop", "Laptop").
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Paired {
    /// Shown once; the device keeps it in its OS keychain.
    pub token: String,
    pub device_id: String,
    pub server_id: String,
    pub epoch: String,
}

fn check_device(req: &PairRequest) -> Result<String, ApiError> {
    if req.device_id.parse::<ulid::Ulid>().is_err() {
        return Err(ApiError::new(ErrorCode::Invalid, "device_id isn't a ULID"));
    }
    let name = req.name.trim();
    if name.is_empty() || name.chars().count() > NAME_MAX {
        return Err(ApiError::new(
            ErrorCode::Invalid,
            format!("the device name must be 1 to {NAME_MAX} characters"),
        ));
    }
    Ok(name.to_owned())
}

/// Trades the open code for a device token. A wrong code counts against
/// it; the fifth wrong guess burns it. Pairing again with the same device
/// id replaces its token and lifts a revocation.
pub fn pair(store: &Store, req: &PairRequest, now: Instant) -> Result<Paired, ApiError> {
    let name = check_device(req)?;
    let conn = store.conn();
    let open: Option<(String, String, i64)> = conn
        .query_row(
            "SELECT code_hash, expires_at, failures FROM pairing WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(sql)?;
    let Some((code_hash, expires_at, failures)) = open else {
        return Err(ApiError::new(
            ErrorCode::PairingClosed,
            "no pairing code is open; run `blotter-sync pair` on the server",
        ));
    };
    if expires_at <= time::fmt_instant(now) {
        conn.execute("DELETE FROM pairing", []).map_err(sql)?;
        return Err(ApiError::new(
            ErrorCode::CodeExpired,
            "that pairing code has expired; make a new one",
        ));
    }
    if hash(&normalize(&req.code)) != code_hash {
        if failures + 1 >= MAX_FAILURES {
            conn.execute("DELETE FROM pairing", []).map_err(sql)?;
            return Err(ApiError::new(
                ErrorCode::CodeExpired,
                "too many wrong codes; that one is used up, make a new one",
            ));
        }
        conn.execute("UPDATE pairing SET failures = failures + 1", [])
            .map_err(sql)?;
        return Err(ApiError::new(
            ErrorCode::BadCode,
            "that pairing code is wrong",
        ));
    }

    let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random_bytes::<32>()?);
    conn.execute("DELETE FROM pairing", []).map_err(sql)?;
    conn.execute(
        "INSERT INTO devices (device_id, name, token_hash, paired_at, last_seen_at) \
         VALUES (?1, ?2, ?3, ?4, ?4) \
         ON CONFLICT (device_id) DO UPDATE SET name = excluded.name, \
         token_hash = excluded.token_hash, paired_at = excluded.paired_at, \
         last_seen_at = excluded.last_seen_at, revoked_at = NULL, clock_ahead_ms = NULL",
        (&req.device_id, &name, hash(&token), time::fmt_instant(now)),
    )
    .map_err(sql)?;
    let meta = store.meta().map_err(ApiError::internal)?;
    Ok(Paired {
        token,
        device_id: req.device_id.clone(),
        server_id: meta.server_id,
        epoch: meta.epoch,
    })
}

/// The device a token belongs to, if it may sync. Marks it seen.
pub fn authenticate(store: &Store, token: &str, now: Instant) -> Result<String, ApiError> {
    let row: Option<(String, Option<String>)> = store
        .conn()
        .query_row(
            "SELECT device_id, revoked_at FROM devices WHERE token_hash = ?1",
            [hash(token)],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(sql)?;
    match row {
        None => Err(ApiError::new(
            ErrorCode::Unauthorized,
            "this server doesn't know that device; pair it again",
        )),
        Some((_, Some(_))) => Err(ApiError::new(
            ErrorCode::Revoked,
            "this device was disconnected from the server; pair it again to sync",
        )),
        Some((device_id, None)) => {
            store
                .conn()
                .execute(
                    "UPDATE devices SET last_seen_at = ?2 WHERE device_id = ?1",
                    (&device_id, time::fmt_instant(now)),
                )
                .map_err(sql)?;
            Ok(device_id)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub device_id: String,
    pub name: String,
    pub paired_at: String,
    pub last_seen_at: Option<String>,
    pub revoked_at: Option<String>,
    pub clock_ahead_ms: Option<i64>,
}

/// Every device ever paired, by name.
pub fn devices(store: &Store) -> Result<Vec<DeviceInfo>, ApiError> {
    let mut stmt = store
        .conn()
        .prepare(
            "SELECT device_id, name, paired_at, last_seen_at, revoked_at, clock_ahead_ms \
             FROM devices ORDER BY name COLLATE NOCASE, paired_at",
        )
        .map_err(sql)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(DeviceInfo {
                device_id: r.get(0)?,
                name: r.get(1)?,
                paired_at: r.get(2)?,
                last_seen_at: r.get(3)?,
                revoked_at: r.get(4)?,
                clock_ahead_ms: r.get(5)?,
            })
        })
        .map_err(sql)?;
    rows.collect::<rusqlite::Result<_>>().map_err(sql)
}

/// Revokes a device by id, or by its name when only one device has it.
/// Returns the device revoked.
pub fn revoke(store: &Store, which: &str, now: Instant) -> Result<DeviceInfo, ApiError> {
    let all = devices(store)?;
    let by_id: Vec<&DeviceInfo> = all.iter().filter(|d| d.device_id == which).collect();
    let matches = if by_id.is_empty() {
        all.iter()
            .filter(|d| d.name.eq_ignore_ascii_case(which.trim()))
            .collect()
    } else {
        by_id
    };
    let device = match matches.as_slice() {
        [one] => (*one).clone(),
        [] => {
            return Err(ApiError::new(
                ErrorCode::Invalid,
                format!("no device is called or has the id {which}"),
            ));
        }
        _ => {
            return Err(ApiError::new(
                ErrorCode::Invalid,
                format!("more than one device is called {which}; use its id"),
            ));
        }
    };
    store
        .conn()
        .execute(
            "UPDATE devices SET revoked_at = COALESCE(revoked_at, ?2) WHERE device_id = ?1",
            (&device.device_id, time::fmt_instant(now)),
        )
        .map_err(sql)?;
    Ok(devices(store)?
        .into_iter()
        .find(|d| d.device_id == device.device_id)
        .unwrap_or(device))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    const DEVICE: &str = "01M5GHBBDWMQRX2Q9VC2GR82XE";

    fn req(code: &str) -> PairRequest {
        PairRequest {
            code: code.into(),
            device_id: DEVICE.into(),
            name: "Laptop".into(),
        }
    }

    #[test]
    fn codes_read_cleanly() {
        for _ in 0..50 {
            let code = new_code().unwrap();
            assert_eq!(code.len(), 9);
            assert_eq!(&code[4..5], "-");
            assert!(
                code.bytes()
                    .filter(|b| *b != b'-')
                    .all(|b| CODE_ALPHABET.contains(&b))
            );
        }
        assert_eq!(normalize(" abcd-efgh "), "ABCDEFGH");
    }

    #[test]
    fn a_code_pairs_once_and_the_token_authenticates() {
        let store = Store::open_in_memory().unwrap();
        let now = Utc::now();
        assert!(!pairing_open(&store, now).unwrap());
        let open = open_pairing(&store, now).unwrap();
        assert!(pairing_open(&store, now).unwrap());

        let paired = pair(
            &store,
            &req(&open.code.to_lowercase().replace('-', "")),
            now,
        )
        .unwrap();
        assert_eq!(paired.token.len(), 43);
        assert_eq!(authenticate(&store, &paired.token, now).unwrap(), DEVICE);
        assert!(!pairing_open(&store, now).unwrap(), "single use");
        let again = pair(&store, &req(&open.code), now).unwrap_err();
        assert_eq!(again.code(), ErrorCode::PairingClosed);

        let stored: String = store
            .conn()
            .query_row("SELECT token_hash FROM devices", [], |r| r.get(0))
            .unwrap();
        assert_ne!(stored, paired.token, "only the hash is kept");
    }

    #[test]
    fn an_expired_code_is_refused() {
        let store = Store::open_in_memory().unwrap();
        let now = Utc::now();
        let open = open_pairing(&store, now).unwrap();
        let later = now + Duration::minutes(CODE_LIFETIME_MIN + 1);
        assert!(!pairing_open(&store, later).unwrap());
        assert_eq!(
            pair(&store, &req(&open.code), later).unwrap_err().code(),
            ErrorCode::CodeExpired
        );
        assert_eq!(
            pair(&store, &req(&open.code), now).unwrap_err().code(),
            ErrorCode::PairingClosed
        );
    }

    #[test]
    fn five_wrong_guesses_burn_the_code() {
        let store = Store::open_in_memory().unwrap();
        let now = Utc::now();
        let open = open_pairing(&store, now).unwrap();
        for _ in 1..MAX_FAILURES {
            assert_eq!(
                pair(&store, &req("WRONG-CODE"), now).unwrap_err().code(),
                ErrorCode::BadCode
            );
        }
        assert_eq!(
            pair(&store, &req("WRONG-CODE"), now).unwrap_err().code(),
            ErrorCode::CodeExpired
        );
        assert_eq!(
            pair(&store, &req(&open.code), now).unwrap_err().code(),
            ErrorCode::PairingClosed
        );
    }

    #[test]
    fn bad_device_details_are_refused() {
        let store = Store::open_in_memory().unwrap();
        let now = Utc::now();
        let open = open_pairing(&store, now).unwrap();
        let mut bad = req(&open.code);
        bad.device_id = "not-a-ulid".into();
        assert_eq!(
            pair(&store, &bad, now).unwrap_err().code(),
            ErrorCode::Invalid
        );
        let mut nameless = req(&open.code);
        nameless.name = "  ".into();
        assert_eq!(
            pair(&store, &nameless, now).unwrap_err().code(),
            ErrorCode::Invalid
        );
        assert!(
            pairing_open(&store, now).unwrap(),
            "the code isn't spent on a bad request"
        );
    }

    #[test]
    fn revoking_stops_the_token_and_pairing_again_lifts_it() {
        let store = Store::open_in_memory().unwrap();
        let now = Utc::now();
        let code = open_pairing(&store, now).unwrap().code;
        let token = pair(&store, &req(&code), now).unwrap().token;
        let revoked = revoke(&store, "laptop", now).unwrap();
        assert!(revoked.revoked_at.is_some());
        assert_eq!(
            authenticate(&store, &token, now).unwrap_err().code(),
            ErrorCode::Revoked
        );

        let code = open_pairing(&store, now).unwrap().code;
        let fresh = pair(&store, &req(&code), now).unwrap().token;
        assert_eq!(authenticate(&store, &fresh, now).unwrap(), DEVICE);
        assert_eq!(
            authenticate(&store, &token, now).unwrap_err().code(),
            ErrorCode::Unauthorized,
            "the old token is gone"
        );
        assert_eq!(devices(&store).unwrap().len(), 1);
    }

    #[test]
    fn revoking_needs_one_clear_device() {
        let store = Store::open_in_memory().unwrap();
        let now = Utc::now();
        assert_eq!(
            revoke(&store, "nobody", now).unwrap_err().code(),
            ErrorCode::Invalid
        );
        for id in ["01M5GHBBDWMQRX2Q9VC2GR82XE", "01M5GHBBKTVTD22JTFVSFRNJFX"] {
            let code = open_pairing(&store, now).unwrap().code;
            let mut r = req(&code);
            r.device_id = id.into();
            pair(&store, &r, now).unwrap();
        }
        assert_eq!(
            revoke(&store, "Laptop", now).unwrap_err().code(),
            ErrorCode::Invalid
        );
        assert!(revoke(&store, "01M5GHBBKTVTD22JTFVSFRNJFX", now).is_ok());
    }

    #[test]
    fn an_unknown_token_is_unauthorized() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(
            authenticate(&store, "nope", Utc::now()).unwrap_err().code(),
            ErrorCode::Unauthorized
        );
    }
}
