use std::collections::BTreeMap;
use rusqlite::Connection;
use crate::{error::{Error, ErrorContext, Result}, writer::Writer};
#[cfg(feature = "reader")]
use crate::reader::Reader;

pub fn build_domain_map<T, F>(conn: &mut Connection, mut f: F) -> Result<BTreeMap<u32, T>>
where
    F: FnMut(Box<[u8]>) -> T
{
    let mut stmt = conn.prepare("SELECT * FROM domains")
        .context("build_domain_map: prepare")?;
    let mut rows = stmt.query([])
        .context("build_domain_map: query")?;
    let mut map = BTreeMap::new();
    while let Some(row) = rows.next().context("build_domain_map: next row")? {
        let id = row.get(0).context("build_domain_map: next domain id")?;
        let name = row.get(1).context("build_domain_map: next domain name")?;
        let name = f(name);
        if map.insert(id, name).is_some() {
            return Err(Error::Invariant("build_domain_map: duplicate domain id"));
        }
    }
    Ok(map)
}

impl Writer {
    pub fn build_domain_map<T, F>(&mut self, f: F) -> Result<BTreeMap<u32, T>>
    where
        F: FnMut(Box<[u8]>) -> T
    {
        build_domain_map(&mut self.conn, f)
    }
}

#[cfg(feature = "reader")]
impl Reader {
    pub fn build_domain_map<T, F>(&mut self, f: F) -> Result<BTreeMap<u32, T>>
    where
        F: FnMut(Box<[u8]>) -> T
    {
        build_domain_map(&mut self.conn, f)
    }
}
