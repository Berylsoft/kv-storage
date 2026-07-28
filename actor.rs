use std::path::PathBuf;
pub use bytes;
use bytes::Bytes;
use actor_core::*;
use super::*;

pub struct KV {
    pub domain: Bytes,
    pub key: Bytes,
    pub value: Bytes,
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
    type Req = KV;
    type Res = ();
    type Err = Error;
}

impl SyncContext for WriterContext {
    fn exec(&mut self, req: KV) -> Result<()> {
        self.writer.write_kv(&req.domain, &req.key, &req.value)
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
