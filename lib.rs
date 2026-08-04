use std::path::Path;
pub use rusqlite;
use rusqlite::{Connection, ffi, params, types::FromSql};
use crc32fast::hash as crc32;
use foundations::const_concat;

pub const MAGIC_A: i32 = 0x42654b56; // BeKV
const SET_MAGIC_A_STMT: &str = "PRAGMA application_id=0x42654b56;";
pub const MAGIC_B: i32 = 0x42654b76; // BeKv
const SET_MAGIC_B_STMT: &str = "PRAGMA application_id=0x42654b76;";
pub const VERSION: u32 = 1;
const SET_VERSION_STMT: &str = "PRAGMA user_version=1;";

const METADATA_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS metadata (
    id INTEGER NOT NULL PRIMARY KEY,
    ident BLOB NOT NULL,
    check (id = 0)
) WITHOUT ROWID;";

const DOMAINS_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS domains (
    domain_id INTEGER NOT NULL PRIMARY KEY,
    domain BLOB NOT NULL
) WITHOUT ROWID;";

const STORAGE_SCHEMA_1: &str = "CREATE TABLE IF NOT EXISTS storage (
    domain_id INTEGER NOT NULL,
    key BLOB NOT NULL,
    value_crc32 INTEGER NOT NULL,
    value BLOB NOT NULL,
    PRIMARY KEY ";
const STORAGE_SCHEMA_2_A: &str = "(domain_id, key)";
const STORAGE_SCHEMA_2_B: &str = "(domain_id, key, value_crc32)";
const STORAGE_SCHEMA_3: &str = ",
    FOREIGN KEY (domain_id) REFERENCES domains(domain_id)
        ON DELETE RESTRICT
        ON UPDATE RESTRICT
) WITHOUT ROWID;";
const STORAGE_SCHEMA_A: &str = const_concat!(STORAGE_SCHEMA_1, STORAGE_SCHEMA_2_A, STORAGE_SCHEMA_3);
const STORAGE_SCHEMA_B: &str = const_concat!(STORAGE_SCHEMA_1, STORAGE_SCHEMA_2_B, STORAGE_SCHEMA_3);

#[derive(Debug)]
pub enum Error {
    Rusqlite(rusqlite::Error, &'static str),
    Invariant(&'static str),
    InvariantUpdatedRowNot0(&'static str),
    DuplicateKey,
    DuplicateValueCrc,
    DuplicateDomain,
    UnknownDomain,
    NotABeKVDatabase,
    VariantNotMatch { exp: bool, cur: bool },
    VersionNotMatch { exp: u32, cur: u32 },
    IdentNotMatch { exp: Box<[u8]>, cur: Box<[u8]> },
    #[cfg(feature = "actor")]
    ActorClosed,
}

trait ErrorContext<T> {
    fn context(self, context: &'static str) -> Result<T>;
}

impl<T> ErrorContext<T> for core::result::Result<T, rusqlite::Error> {
    fn context(self, context: &'static str) -> Result<T> {
        self.map_err(|err| Error::Rusqlite(err, context))
    }
}

pub type Result<T> = core::result::Result<T, Error>;

fn make_rusqlite_result(result_code: core::ffi::c_int, msg: &str) -> rusqlite::Result<()> {
    match result_code {
        ffi::SQLITE_OK => Ok(()),
        result_code => Err(rusqlite::Error::SqliteFailure(
            ffi::Error::new(result_code),
            Some(msg.to_owned()),
        )),
    }
}

fn register_cksumvfs() -> rusqlite::Result<()> {
    let result_code = unsafe {
        ffi::sqlite3_register_cksumvfs(std::ptr::null())
    };
    make_rusqlite_result(
        result_code,
        "error calling sqlite3_register_cksumvfs",
    )
}

fn set_reserve_bytes(conn: &Connection) -> rusqlite::Result<()> {
    let mut reserve_bytes = 8;
    let result_code = unsafe {
        ffi::sqlite3_file_control(
            conn.handle(),
            std::ptr::null(),
            ffi::SQLITE_FCNTL_RESERVE_BYTES,
            (&mut reserve_bytes) as *mut i32 as *mut core::ffi::c_void,
        )
    };
    make_rusqlite_result(
        result_code,
        "error calling sqlite3_file_control to set reserve bytes as 8",
    )
}

fn run_and_check_update_rows(conn: &Connection, stmt: &str, msg: &'static str) -> Result<()> {
    let updated_rows = conn.execute(stmt, []).context(msg)?;
    if updated_rows == 0 {
        Ok(())
    } else {
        Err(Error::InvariantUpdatedRowNot0(msg))
    }
}

fn query_one_row<T: FromSql>(conn: &Connection, stmt: &str) -> rusqlite::Result<T> {
    conn.query_one(stmt, [], |r| r.get::<_, T>(0))
}

fn run_vacuum(conn: &Connection) -> Result<()> {
    run_and_check_update_rows(conn, "vacuum;", "run vacuum")
}

fn ensure_checksum_enabled(conn: &Connection) -> Result<()> {
    let enabled: String = query_one_row(conn, "PRAGMA checksum_verification;")
        .context("check checksum_verification")?;
    if enabled != "1" {
        return Err(Error::Invariant("checksum_verification not enabled"));
    }
    Ok(())
}

fn set_synchronous(conn: &Connection) -> Result<()> {
    run_and_check_update_rows(
        conn,
        "PRAGMA synchronous = EXTRA;",
        "set synchronous",
    )?;
    run_and_check_update_rows(
        conn,
        "PRAGMA fullfsync = true;",
        "set fullfsync",
    )
}

fn enable_foreign_keys(conn: &Connection) -> Result<()> {
    run_and_check_update_rows(
        conn,
        "PRAGMA foreign_keys = ON;",
        "enable foreign_keys"
    )
}

fn check_if_database_is_new(conn: &Connection) -> Result<bool> {
    let count: u32 = query_one_row(
        conn,
        "SELECT count(*) FROM sqlite_master WHERE type='table';"
    ).context("check if database is new")?;
    Ok(count == 0)
}

fn check_or_write_version(conn: &Connection, deny_dup_key: bool) -> Result<()> {
    let magic: i32 = query_one_row(conn, "PRAGMA application_id;").context("get magic")?;
    let version: u32 = query_one_row(conn, "PRAGMA user_version;").context("get version")?;
    let database_is_new = check_if_database_is_new(conn)?;
    match (database_is_new, magic, version, deny_dup_key) {
        (true, 0, 0, _) => {
            run_and_check_update_rows(
                conn,
                if deny_dup_key { SET_MAGIC_A_STMT } else { SET_MAGIC_B_STMT },
                "set magic",
            )?;
            run_and_check_update_rows(
                conn,
                SET_VERSION_STMT,
                "set version",
            )
        }
        (false, MAGIC_A, VERSION, true) |
        (false, MAGIC_B, VERSION, false) => {
            Ok(())
        }
        (false, MAGIC_A, _, cur @ false) => {
            Err(Error::VariantNotMatch { exp: deny_dup_key, cur })
        }
        (false, MAGIC_B, _, cur @ true) => {
            Err(Error::VariantNotMatch { exp: deny_dup_key, cur })
        }
        (false, MAGIC_A | MAGIC_B, cur, _) => {
            Err(Error::VersionNotMatch { exp: VERSION, cur })
        }
        _ => {
            Err(Error::NotABeKVDatabase)
        }
    }
}

fn init_schema(conn: &Connection, deny_dup_key: bool) -> Result<()> {
    run_and_check_update_rows(
        conn,
        METADATA_SCHEMA,
        "init metadata schema",
    )?;
    run_and_check_update_rows(
        conn,
        DOMAINS_SCHEMA,
        "init domains schema",
    )?;
    run_and_check_update_rows(
        conn,
        if deny_dup_key { STORAGE_SCHEMA_A } else { STORAGE_SCHEMA_B },
        "init storage schema",
    )
}

struct Metadata {
    ident: Box<[u8]>,
}

fn check_or_write_metadata(conn: &Connection, ident: &[u8]) -> Result<()> {
    let mut stmt = conn.prepare("SELECT * FROM metadata")
        .context("check metadata: prepare")?;
    let mut metadata_iter = stmt.query_map([], |row| {
        Ok(Metadata {
            ident: row.get(1)?,
        })
    }).context("check metadata: query")?;
    match metadata_iter.next() {
        None => {
            let updated_rows = conn.execute(
                "INSERT INTO metadata (id, ident) VALUES (?, ?)",
                params![0, ident],
            ).context("write metadata")?;
            if updated_rows != 1 {
                return Err(Error::Invariant("write_metadata updated_rows not 1"));
            }
        }
        Some(cur) => {
            let cur = cur.context("check metadata: get")?;
            if ident != cur.ident.as_ref() {
                return Err(Error::IdentNotMatch {
                    exp: ident.into(),
                    cur: cur.ident,
                });
            }
        }
    }
    if metadata_iter.next().is_some() {
        return Err(Error::Invariant("more than 1 rows in metadata table"));
    }
    Ok(())
}

pub struct Writer {
    conn: Connection,
    deny_dup_key: bool,
}

impl Writer {
    pub fn open(path: impl AsRef<Path>, deny_dup_key: bool, ident: &[u8]) -> Result<Self> {
        register_cksumvfs().context("register cksumvfs")?;
        let conn = Connection::open(path).context("open file")?;
        set_reserve_bytes(&conn).context("set reserve bytes")?;
        run_vacuum(&conn)?;
        set_synchronous(&conn)?;
        enable_foreign_keys(&conn)?;
        ensure_checksum_enabled(&conn)?;
        check_or_write_version(&conn, deny_dup_key)?;
        init_schema(&conn, deny_dup_key)?;
        check_or_write_metadata(&conn, ident)?;
        Ok(Self { conn, deny_dup_key })
    }

    pub fn write_domain(&mut self, domain_id: u32, domain: &[u8]) -> Result<()> {
        let tr = self.conn.transaction().context("write domain: begin transaction")?;

        let domain_exists: bool = tr.query_one(
            "SELECT EXISTS (SELECT 1 FROM domains WHERE domain_id = ?)",
            params![domain_id], |r| r.get(0),
        ).context("write domain: check if domain exists")?;
        if domain_exists {
            return Err(Error::DuplicateDomain);
        }

        let updated_rows = tr.execute(
            "INSERT INTO domains (domain_id, domain) VALUES (?, ?)",
            params![domain_id, domain],
        ).context("write domain: write")?;
        if updated_rows != 1 {
            return Err(Error::Invariant("write domain updated_rows not 1"));
        }

        tr.commit().context("write domain: commit")?;
        Ok(())
    }

    pub fn write_kv(&mut self, domain_id: u32, key: &[u8], value: &[u8]) -> Result<()> {
        let value_crc32 = crc32(value);
        let tr = self.conn.transaction().context("write kv: begin transaction")?;

        let domain_exists: bool = tr.query_one(
            "SELECT EXISTS (SELECT 1 FROM domains WHERE domain_id = ?)",
            params![domain_id], |r| r.get(0),
        ).context("write kv: check if domain exists")?;
        if !domain_exists {
            return Err(Error::UnknownDomain);
        }

        if self.deny_dup_key {
            let key_exists: bool = tr.query_one(
                "SELECT EXISTS (SELECT 1 FROM storage WHERE (domain_id, key) = (?, ?))",
                params![domain_id, key], |r| r.get(0),
            ).context("write kv: check if key exists")?;
            if key_exists {
                return Err(Error::DuplicateKey);
            }
        } else {
            let key_value_exists: bool = tr.query_one(
                "SELECT EXISTS (SELECT 1 FROM storage WHERE (domain_id, key, value_crc32) = (?, ?, ?))",
                params![domain_id, key, value_crc32], |r| r.get(0),
            ).context("write kv: check if key-value exists")?;
            if key_value_exists {
                return Err(Error::DuplicateValueCrc);
            }
        }

        let updated_rows = tr.execute(
            "INSERT INTO storage (domain_id, key, value_crc32, value) VALUES (?, ?, ?, ?)",
            params![domain_id, key, value_crc32, value],
        ).context("write kv: write")?;
        if updated_rows != 1 {
            return Err(Error::Invariant("write kv updated_rows not 1"));
        }

        tr.commit().context("write kv: commit")?;
        Ok(())
    }

    pub fn close(self) -> Result<()> {
        match self.conn.close() {
            Ok(()) => Ok(()),
            Err((_conn, err)) => Err(Error::Rusqlite(err, "close file")),
        }
    }
}

#[cfg(feature = "actor")]
pub mod actor;
