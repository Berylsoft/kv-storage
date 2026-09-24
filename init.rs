use std::sync::atomic::{self, AtomicBool};
use rusqlite::{Connection, ffi, params, types::FromSql};
use crate::{
    MAGIC, SET_MAGIC_STMT, VERSION, SET_VERSION_STMT,
    METADATA_SCHEMA, DOMAINS_SCHEMA, STORAGE_SCHEMA,
    Metadata,
    error::{Error, Result, ErrorContext, make_rusqlite_result},
};

fn register_cksumvfs() -> rusqlite::Result<()> {
    let result_code = unsafe {
        ffi::sqlite3_register_cksumvfs(std::ptr::null())
    };
    make_rusqlite_result(
        result_code,
        "error calling sqlite3_register_cksumvfs",
    )
}

pub fn register_cksumvfs_once() -> rusqlite::Result<()> {
    static REGISTER_RUNNING: AtomicBool = AtomicBool::new(false);
    static REGISTER_DONE: AtomicBool = AtomicBool::new(false);

    while !REGISTER_DONE.load(atomic::Ordering::Acquire) {
        struct Guard;
        impl Drop for Guard {
            fn drop(&mut self) {
                REGISTER_RUNNING.store(false, atomic::Ordering::Release);
            }
        }

        let running = REGISTER_RUNNING.compare_exchange(
            false,
            true,
            atomic::Ordering::AcqRel,
            atomic::Ordering::Relaxed,
        );
        if running.is_err() {
            std::hint::spin_loop();
            continue;
        }
        let guard = Guard;

        if REGISTER_DONE.load(atomic::Ordering::Acquire) {
            break;
        }

        register_cksumvfs()?;

        REGISTER_DONE.store(true, atomic::Ordering::Release);
        drop(guard);
    }

    Ok(())
}

pub fn set_reserve_bytes(conn: &Connection) -> rusqlite::Result<()> {
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

pub fn run_vacuum(conn: &Connection) -> Result<()> {
    run_and_check_update_rows(conn, "vacuum;", "run vacuum")
}

pub fn set_synchronous(conn: &Connection) -> Result<()> {
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

pub fn enable_foreign_keys(conn: &Connection) -> Result<()> {
    run_and_check_update_rows(
        conn,
        "PRAGMA foreign_keys = ON;",
        "enable foreign_keys"
    )
}

fn query_one_row<T: FromSql>(conn: &Connection, stmt: &str) -> rusqlite::Result<T> {
    conn.query_one(stmt, [], |r| r.get::<_, T>(0))
}

pub fn ensure_checksum_enabled(conn: &Connection) -> Result<()> {
    let enabled: String = query_one_row(conn, "PRAGMA checksum_verification;")
        .context("check checksum_verification")?;
    if enabled != "1" {
        return Err(Error::Invariant("checksum_verification not enabled"));
    }
    Ok(())
}

fn check_if_database_is_new(conn: &Connection) -> Result<bool> {
    let count: u32 = query_one_row(
        conn,
        "SELECT count(*) FROM sqlite_master WHERE type='table';"
    ).context("check if database is new")?;
    Ok(count == 0)
}

pub fn check_or_write_version(conn: &Connection) -> Result<()> {
    let magic: i32 = query_one_row(conn, "PRAGMA application_id;").context("get magic")?;
    let version: u32 = query_one_row(conn, "PRAGMA user_version;").context("get version")?;
    let database_is_new = check_if_database_is_new(conn)?;
    match (database_is_new, magic, version) {
        (true, 0, 0) => {
            run_and_check_update_rows(
                conn,
                SET_MAGIC_STMT,
                "set magic",
            )?;
            run_and_check_update_rows(
                conn,
                SET_VERSION_STMT,
                "set version",
            )
        }
        (false, MAGIC, VERSION) => {
            Ok(())
        }
        (false, MAGIC, cur_version) => {
            Err(Error::VersionNotMatch { exp: VERSION, cur: cur_version })
        }
        _ => {
            Err(Error::NotABeKVDatabase)
        }
    }
}

pub fn check_version(conn: &Connection) -> Result<()> {
    let magic: i32 = query_one_row(conn, "PRAGMA application_id;").context("get magic")?;
    let version: u32 = query_one_row(conn, "PRAGMA user_version;").context("get version")?;
    let database_is_new = check_if_database_is_new(conn)?;
    match (database_is_new, magic, version) {
        (false, MAGIC, VERSION) => {
            Ok(())
        }
        (false, MAGIC, cur_version) => {
            Err(Error::VersionNotMatch { exp: VERSION, cur: cur_version })
        }
        _ => {
            // including empty database
            Err(Error::NotABeKVDatabase)
        }
    }
}

pub fn init_schema(conn: &Connection) -> Result<()> {
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
        STORAGE_SCHEMA,
        "init storage schema",
    )
}

pub fn check_or_write_metadata(conn: &Connection, metadata: Metadata) -> Result<()> {
    let mut stmt = conn.prepare("SELECT * FROM metadata")
        .context("check metadata: prepare")?;
    let mut metadata_iter = stmt.query_map([], |row| {
        Ok(Metadata {
            ident: row.get(1)?,
        })
    }).context("check metadata: query")?;
    match metadata_iter.next() {
        None => {
            // TODO: what if a not-new database has 0 row in metadata table?
            let updated_rows = conn.execute(
                "INSERT INTO metadata (id, ident) VALUES (?, ?)",
                params![0, metadata.ident],
            ).context("write metadata")?;
            if updated_rows != 1 {
                return Err(Error::Invariant("write metadata updated_rows not 1"));
            }
        }
        Some(cur) => {
            let cur = cur.context("check metadata: get")?;
            if metadata.ident != cur.ident {
                return Err(Error::IdentNotMatch {
                    exp: metadata.ident,
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

pub fn read_metadata(conn: &Connection) -> Result<Metadata> {
    let mut stmt = conn.prepare("SELECT * FROM metadata")
        .context("read metadata: prepare")?;
    let mut metadata_iter = stmt.query_map([], |row| {
        Ok(Metadata {
            ident: row.get(1)?,
        })
    }).context("read metadata: query")?;
    let res = match metadata_iter.next() {
        None => {
            return Err(Error::Invariant("0 row in metadata table when read metadata"));
        }
        Some(cur) => {
            cur.context("read metadata: get")?
        }
    };
    if metadata_iter.next().is_some() {
        return Err(Error::Invariant("more than 1 rows in metadata table"));
    }
    Ok(res)
}
