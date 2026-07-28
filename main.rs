use kv_storage::WriteContext;

fn main() {
    let ctx = WriteContext::open("temp.db").unwrap();
    ctx.write_kv(b"domain", b"1", b"value1").unwrap();
    ctx.write_kv(b"domain", b"2", b"value2").unwrap();
}
