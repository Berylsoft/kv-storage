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
}
