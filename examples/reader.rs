use kv_storage::reader::Reader;

fn main() {
    let (mut ctx, metadata) = Reader::open("temp.db").unwrap();
    assert_eq!(metadata.ident.as_ref(), b"test");

    assert_eq!(
        ctx.get_domain_name_by_id(1).unwrap().as_ref(),
        b"domain",
    );
    assert_eq!(
        ctx.get_domain_id_by_name(b"domain").unwrap(),
        1,
    );
    assert_eq!(
        ctx.get_value_by_key(1, b"1").unwrap().as_ref(),
        b"value1",
    );
    assert_eq!(
        ctx.get_key_by_value(1, b"value1").unwrap().as_ref(),
        b"1",
    );
}
