use bytes::Bytes;
use actor_core::*;
use crate::{error::{Error, Result}, writer::*};
use super::{Domain, KV, WriterContextConfig};

pub enum Request {
    Domain(Bytes),
    KV(KV),
}

pub enum Response {
    Domain(u32),
    KV,
}

pub struct WriterContext {
    writer: Writer,
}

impl Context for WriterContext {
    type Req = Request;
    type Res = Response;
    type Err = Error;
}

impl SyncContext for WriterContext {
    fn exec(&mut self, req: Request) -> Result<Response> {
        Ok(match req {
            Request::Domain(domain) => {
                let domain_id = match self.writer.get_domain_id_by_name(&domain)? {
                    Some(domain_id) => domain_id,
                    None => self.writer.get_max_domain_id()?
                        .checked_add(1).ok_or_else(|| Error::DomainIdExhausted)?,
                };
                self.writer.write_domain(domain_id, &domain)?;
                Response::Domain(domain_id)
            }
            Request::KV(KV { domain_id, key, value }) => {
                self.writer.write_kv(domain_id, &key, &value)?;
                Response::KV
            }
        })
    }

    fn close(self) -> Result<()> {
        self.writer.close()
    }
}

impl SyncInitContext for WriterContext {
    type Init = WriterContextConfig;

    fn init(WriterContextConfig { path, metadata }: WriterContextConfig) -> Result<Self> {
        let writer = Writer::open(path, metadata)?;
        Ok(WriterContext { writer })
    }
}


pub struct WriteHandle {
    tx: actor::Handle<WriterContext>,
}

pub struct DomainWriteHandle {
    tx: actor::Handle<WriterContext>,
    domain: Domain,
}

impl WriteHandle {
    pub async fn spawn_domain(&self, domain: Bytes) -> Result<DomainWriteHandle> {
        let res = self.tx.request(Request::Domain(domain.clone())).await?;
        let domain_id = match res {
            Response::Domain(domain_id) => domain_id,
            _ => return Err(Error::Invariant("actor: wrong response type")),
        };
        let domain = Domain { domain_id, domain };
        Ok(DomainWriteHandle {
            tx: self.tx.clone(),
            domain,
        })
    }

    pub async fn wait_close(self) -> Result<()> {
        self.tx.wait_close().await
    }
}

impl DomainWriteHandle {
    pub async fn write_kv(&self, key: Bytes, value: Bytes) -> Result<()> {
        let res = self.tx.request(Request::KV(KV {
            domain_id: self.domain.domain_id,
            key,
            value,
        })).await?;
        match res {
            Response::KV => Ok(()),
            _ => Err(Error::Invariant("actor: wrong response type"))
        }
    }

    pub fn domain_id(&self) -> u32 {
        self.domain.domain_id
    }

    pub fn domain(&self) -> Bytes {
        self.domain.domain.clone()
    }
}

pub async fn create(config: WriterContextConfig) -> Result<WriteHandle> {
    let tx = actor::create_sync_sync(config).await?;
    Ok(WriteHandle { tx })
}
