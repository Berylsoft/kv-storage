use std::assert_matches;
use kv_storage::{
    Result, Error,
    actor::{WriterContextConfig},
    actor_domain_handle::{create, DomainWriteHandle},
};

fn b(bytes: &'static [u8]) -> bytes::Bytes {
    bytes::Bytes::from_static(bytes)
}

fn spawn_domain_unwrap_err(res: Result<DomainWriteHandle>) -> Error {
    match res {
        Err(err) => err,
        Ok(_) => panic!("called `Result::unwrap_err()` on an `Ok` value")
    }
}

fn main() {
    async_global_executor::block_on(async {
        let config = WriterContextConfig {
            path: "temp.db".into(),
            ident: "test".into(),
        };
        let handle = create(config).await.unwrap();
        let domain_1 = handle.spawn_domain(1, b(b"domain")).await.unwrap();
        domain_1.write_kv(b(b"1"), b(b"value1")).await.unwrap();
        domain_1.write_kv(b(b"2"), b(b"value2")).await.unwrap();
        assert_matches!(
            spawn_domain_unwrap_err(handle.spawn_domain(1, b(b"domain")).await),
            Error::DuplicateDomain,
        );
        assert_matches!(
            domain_1.write_kv(b(b"1"), b(b"value3")).await.unwrap_err(),
            Error::DuplicateKey,
        );
    })
}
