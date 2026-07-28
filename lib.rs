use rusqlite::{Connection, ffi, params};
use std::path::Path;

pub const VERSION: i64 = 1;

#[derive(Debug)]
pub enum Error {
    Rusqlite(rusqlite::Error),
    Invariant(&'static str),
    DuplicateKey(rusqlite::Error),
    Version { exp: i64, cur: i64 },
    Ident { exp: Box<[u8]>, cur: Box<[u8]> },
    #[cfg(feature = "actor")]
    ActorClosed,
}

impl From<rusqlite::Error> for Error {
    fn from(value: rusqlite::Error) -> Self {
        Error::Rusqlite(value)
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
        "error calling sqlite3_register_cksumvfs"
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

fn check_if_updated_rows_zero(result: rusqlite::Result<usize>, msg: &'static str) -> Result<()> {
    let updated_rows = result?;
    if updated_rows == 0 {
        Ok(())
    } else {
        Err(Error::Invariant(msg))
    }
}

fn run_vacuum(conn: &Connection) -> Result<()> {
    check_if_updated_rows_zero(
        conn.execute("vacuum;", []),
        "run vacuum updated_rows not 0",
    )
}

fn ensure_checksum_enabled(conn: &Connection) -> Result<()> {
    let enabled: String = conn.query_row(
        "PRAGMA checksum_verification;", [],
        |r| r.get(0),
    )?;
    if enabled != "1" {
        return Err(Error::Invariant("checksum_verification not enabled"));
    }
    Ok(())
}

fn set_synchronous(conn: &Connection) -> Result<()> {
    check_if_updated_rows_zero(
        conn.execute("PRAGMA synchronous = EXTRA;", []),
        "set synchronous updated_rows not 0",
    )?;
    check_if_updated_rows_zero(
        conn.execute("PRAGMA fullfsync = true;", []),
        "set fullfsync updated_rows not 0",
    )
}

fn init_schema(conn: &Connection) -> Result<()> {
    let metadata_schema = "CREATE TABLE IF NOT EXISTS metadata (
        id INTEGER PRIMARY KEY,
        version INTEGER NOT NULL,
        ident BLOB NOT NULL,
        check (id = 1)
    );";
    let storage_schema = "CREATE TABLE IF NOT EXISTS kv_storage (
        domain BLOB NOT NULL,
        key BLOB NOT NULL,
        value BLOB NOT NULL,
        PRIMARY KEY (domain, key)
    ) WITHOUT ROWID;";
    check_if_updated_rows_zero(
        conn.execute(metadata_schema, []),
        "init metadata schema updated_rows not 0",
    )?;
    check_if_updated_rows_zero(
        conn.execute(storage_schema, []),
        "init stroage schema updated_rows not 0",
    )
}

struct Metadata {
    version: i64,
    ident: Box<[u8]>,
}

fn write_metadata(conn: &Connection, ident: &[u8]) -> Result<()> {
    let mut stmt = conn.prepare("SELECT * FROM metadata WHERE id = 1")?;
    let mut metadata_iter = stmt.query_map([], |row| {
        Ok(Metadata {
            version: row.get(1)?,
            ident: row.get(2)?,
        })
    })?;
    match metadata_iter.next() {
        None => {
            let updated_rows = conn.execute(
                "INSERT INTO metadata (version, ident) VALUES (?, ?)",
                params![VERSION, ident],
            )?;
            if updated_rows != 1 {
                return Err(Error::Invariant("write_metadata updated_rows not 1"));
            }
        }
        Some(cur) => {
            let cur = cur?;
            if VERSION != cur.version {
                return Err(Error::Version {
                    exp: VERSION,
                    cur: cur.version,
                });
            }
            if ident != cur.ident.as_ref() {
                return Err(Error::Ident {
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

fn filter_error_duplicate_key(err: rusqlite::Error) -> Error {
    if let rusqlite::Error::SqliteFailure(code, _msg) = &err
        && code.extended_code == 1555
        // && let Some(msg) = msg
        // && msg == "UNIQUE constraint failed: kv_storage.domain, kv_storage.key"
    {
        Error::DuplicateKey(err)
    } else {
        Error::Rusqlite(err)
    }
}

pub struct Writer {
    conn: Connection,
}

impl Writer {
    pub fn open(path: impl AsRef<Path>, ident: &[u8]) -> Result<Self> {
        register_cksumvfs()?;
        let conn = Connection::open(path)?;
        set_reserve_bytes(&conn)?;
        run_vacuum(&conn)?;
        set_synchronous(&conn)?;
        ensure_checksum_enabled(&conn)?;
        init_schema(&conn)?;
        write_metadata(&conn, ident)?;
        Ok(Self { conn })
    }

    pub fn write_kv(&self, domain: &[u8], key: &[u8], value: &[u8]) -> Result<()> {
        let result = self.conn.execute(
            "INSERT INTO kv_storage (domain, key, value) VALUES (?, ?, ?)",
            params![domain, key, value],
        );
        match result {
            Ok(updated_rows) => {
                if updated_rows != 1 {
                    return Err(Error::Invariant("write_kv updated_rows not 1"));
                }
                Ok(())
            }
            Err(err) => Err(filter_error_duplicate_key(err)),
        }
    }

    pub fn close(self) -> Result<()> {
        match self.conn.close() {
            Ok(()) => Ok(()),
            Err((_conn, err)) => Err(err.into()),
        }
    }
}

#[cfg(feature = "actor")]
pub mod actor;
