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

    fn check(k: Box<[u8]>, v: Box<[u8]>, i: u32) {
        let k = u32::from_be_bytes(k.as_ref().try_into().unwrap());
        let v = u32::from_be_bytes(v.as_ref().try_into().unwrap());
        assert_eq!(k, i);
        assert_eq!(v, i + 1);
    }
    let mut i = 500u32;
    ctx.iter_range(
        3,
        i.to_be_bytes().as_ref(),
        (i + 200).to_be_bytes().as_ref(),
        |k, v| {
            check(k, v, i);
            i += 1;
        }
    ).unwrap();
    i = 0;
    ctx.iter(
        3,
        |k, v| {
            check(k, v, i);
            i += 1;
        }
    ).unwrap();
}
