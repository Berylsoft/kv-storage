use std::assert_matches;
use kv_storage::{Error, Writer};

fn main() {
    let ctx = Writer::open("temp.db", b"test").unwrap();
    ctx.write_domain(1, b"domain").unwrap();
    ctx.write_kv(1, b"1", b"value1").unwrap();
    ctx.write_kv(1, b"2", b"value2").unwrap();
    assert_matches!(ctx.write_domain(1, b"domain1"), Err(Error::DuplicateDomainId(_)));
    assert_matches!(ctx.write_kv(1, b"1", b"value3"), Err(Error::DuplicateKey(_)));
    assert_matches!(ctx.write_kv(2, b"key", b"value"), Err(Error::UnknownDomainId(_)));
}
