use std::path::PathBuf;
pub use bytes;
use bytes::Bytes;
use actor_core::*;
use super::*;

pub struct Domain {
    pub domain_id: i64,
    pub domain: Bytes,
}

pub struct KV {
    pub domain_id: i64,
    pub key: Bytes,
    pub value: Bytes,
}

pub enum Request {
    Domain(Domain),
    KV(KV),
}

impl From<ClosedError> for Error {
    fn from(_: ClosedError) -> Self {
        Error::ActorClosed
    }
}

pub struct WriterContextConfig {
    path: PathBuf,
    ident: Bytes,
}

pub struct WriterContext {
    writer: Writer,
}

impl Context for WriterContext {
    type Req = Request;
    type Res = ();
    type Err = Error;
}

impl SyncContext for WriterContext {
    fn exec(&mut self, req: Request) -> Result<()> {
        match req {
            Request::Domain(Domain { domain_id, domain }) => {
                self.writer.write_domain(domain_id, &domain)
            }
            Request::KV(KV { domain_id, key, value }) => {
                self.writer.write_kv(domain_id, &key, &value)
            }
        }
    }

    fn close(self) -> Result<()> {
        self.writer.close()
    }
}

impl SyncInitContext for WriterContext {
    type Init = WriterContextConfig;

    fn init(init: WriterContextConfig) -> Result<Self> {
        let writer = Writer::open(init.path, &init.ident)?;
        Ok(WriterContext { writer })
    }
}
