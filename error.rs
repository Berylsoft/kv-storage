use rusqlite::ffi;

#[derive(Debug)]
pub enum Error {
    Rusqlite(rusqlite::Error, &'static str),
    Invariant(&'static str),
    InvariantUpdatedRowNot0(&'static str),
    DuplicateKey,
    DuplicateDomainId,
    DuplicateDomainName,
    UnknownDomainId,
    NotABeKVDatabase,
    VersionNotMatch { exp: u32, cur: u32 },
    IdentNotMatch { exp: Box<[u8]>, cur: Box<[u8]> },
    #[cfg(feature = "actor")]
    ActorClosed,
}

pub(crate) trait ErrorContext<T> {
    fn context(self, context: &'static str) -> Result<T>;
}

impl<T> ErrorContext<T> for core::result::Result<T, rusqlite::Error> {
    fn context(self, context: &'static str) -> Result<T> {
        self.map_err(|err| Error::Rusqlite(err, context))
    }
}

pub type Result<T> = core::result::Result<T, Error>;

pub(crate) fn make_rusqlite_result(result_code: core::ffi::c_int, msg: &str) -> rusqlite::Result<()> {
    match result_code {
        ffi::SQLITE_OK => Ok(()),
        result_code => Err(rusqlite::Error::SqliteFailure(
            ffi::Error::new(result_code),
            Some(msg.to_owned()),
        )),
    }
}
