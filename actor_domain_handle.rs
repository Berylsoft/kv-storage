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
    pub async fn spawn_domain(&self, domain_id: u32, domain: Bytes) -> Result<DomainWriteHandle> {
        let domain = Domain { domain_id, domain };
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
