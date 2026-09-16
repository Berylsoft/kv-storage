use bytes::Bytes;
use crate::{Result, actor::*};

pub struct WriteHandle {
    tx: actor::Handle<WriterContext>,
}

pub struct DomainWriteHandle {
    tx: actor::Handle<WriterContext>,
    domain: Domain,
}

impl WriteHandle {
    pub async fn spawn_domain(&self, domain: Domain) -> Result<DomainWriteHandle> {
        self.tx.request(Request::Domain(domain.clone())).await?;
        Ok(DomainWriteHandle {
            tx: self.tx.clone(),
            domain,
        })
    }
}

impl DomainWriteHandle {
    pub async fn write_kv(&self, key: Bytes, value: Bytes) -> Result<()> {
        self.tx.request(Request::KV(KV {
            domain_id: self.domain.domain_id,
            key,
            value,
        })).await
    }
}
