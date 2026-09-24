use std::assert_matches;
use kv_storage::{Metadata, error::Error, writer::Writer};

fn main() {
    let metadata = Metadata {
        ident: b"test".as_ref().into()
    };
    let mut ctx = Writer::open("temp.db", metadata).unwrap();
    ctx.write_domain(1, b"domain").unwrap();
    ctx.write_kv(1, b"1", b"value1").unwrap();
    ctx.write_kv(1, b"2", b"value2").unwrap();
    assert_matches!(
        ctx.write_domain(1, b"domain1").unwrap_err(),
        Error::DuplicateDomainId,
    );
    assert_matches!(
        ctx.write_kv(1, b"1", b"value3").unwrap_err(),
        Error::DuplicateKey,
    );
    assert_matches!(
        ctx.write_kv(2, b"key", b"value").unwrap_err(),
        Error::UnknownDomainId,
    );
    assert_matches!(
        ctx.write_domain(2, b"domain").unwrap_err(),
        Error::DuplicateDomainName,
    );
    ctx.write_domain(2, b"domain2").unwrap();
    ctx.write_kv(2, b"1", b"value1").unwrap();

    ctx.write_domain(3, b"batch_test").unwrap();
    for i in 0..1000u32 {
        ctx.write_kv(3, i.to_be_bytes().as_ref(), (i + 1).to_be_bytes().as_ref()).unwrap();
    }
}
