pub mod deps {
    pub use rusqlite;

    #[cfg(feature = "actor")]
    pub use bytes;
    #[cfg(feature = "actor")]
    pub use actor_core;

    #[cfg(feature = "actor-domain-handle")]
    pub use actor;
}

pub const MAGIC: i32 = 0x42654b56; // BeKV
const SET_MAGIC_STMT: &str = "PRAGMA application_id=0x42654b56;";
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

const STORAGE_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS storage (
    domain_id INTEGER NOT NULL,
    key BLOB NOT NULL,
    value BLOB NOT NULL,
    PRIMARY KEY (domain_id, key)
    FOREIGN KEY (domain_id) REFERENCES domains(domain_id)
        ON DELETE RESTRICT
        ON UPDATE RESTRICT
) WITHOUT ROWID;";

pub struct Metadata {
    pub ident: Box<[u8]>,
}

pub mod error;

pub(crate) mod init;

pub mod writer;
