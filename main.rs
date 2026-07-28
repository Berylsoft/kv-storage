use kv_storage::Writer;

fn main() {
    let ctx = Writer::open("temp.db", b"test").unwrap();
    ctx.write_kv(b"domain", b"1", b"value1").unwrap();
    ctx.write_kv(b"domain", b"2", b"value2").unwrap();
}
