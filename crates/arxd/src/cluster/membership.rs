//! Canonical membership-v1 structure, independent of storage and consensus engines.
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MembershipError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Membership {
    configs: Vec<BTreeSet<u64>>,
    nodes: BTreeMap<u64, String>,
}

fn valid_address(a: &str) -> bool {
    (1..=200).contains(&a.len())
        && a.bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._:[]-".contains(&c))
}

impl Membership {
    pub(crate) fn new(
        configs: Vec<BTreeSet<u64>>,
        nodes: BTreeMap<u64, String>,
    ) -> Result<Self, MembershipError> {
        if !(1..=2).contains(&configs.len())
            || !(1..=16).contains(&nodes.len())
            || configs.iter().any(|c| {
                !(1..=16).contains(&c.len()) || c.iter().any(|n| *n == 0 || !nodes.contains_key(n))
            })
            || nodes.iter().any(|(n, a)| *n == 0 || !valid_address(a))
        {
            return Err(MembershipError);
        }
        Ok(Self { configs, nodes })
    }

    pub(crate) fn configs(&self) -> &[BTreeSet<u64>] {
        &self.configs
    }
    pub(crate) fn nodes(&self) -> &BTreeMap<u64, String> {
        &self.nodes
    }
    pub(crate) fn into_parts(self) -> (Vec<BTreeSet<u64>>, BTreeMap<u64, String>) {
        (self.configs, self.nodes)
    }

    pub(crate) fn encode(&self) -> Result<Vec<u8>, MembershipError> {
        let mut b = vec![1, self.configs().len() as u8];
        for c in self.configs() {
            b.push(c.len() as u8);
            for n in c {
                b.extend(n.to_be_bytes());
            }
        }
        b.push(self.nodes().len() as u8);
        for (n, a) in self.nodes() {
            b.extend(n.to_be_bytes());
            b.push(a.len() as u8);
            b.extend(a.as_bytes());
        }
        if b.len() > 4096 {
            return Err(MembershipError);
        }
        Ok(b)
    }

    pub(crate) fn decode(b: &[u8]) -> Result<Self, MembershipError> {
        if b.is_empty() || b.len() > 4096 {
            return Err(MembershipError);
        }
        let mut r = Reader(b);
        if r.byte()? != 1 {
            return Err(MembershipError);
        }
        let count = r.byte()?;
        if !(1..=2).contains(&count) {
            return Err(MembershipError);
        }
        let mut configs = Vec::new();
        for _ in 0..count {
            let n = r.byte()?;
            if !(1..=16).contains(&n) {
                return Err(MembershipError);
            }
            let mut c = BTreeSet::new();
            let mut prev = 0;
            for _ in 0..n {
                let v = r.u64()?;
                if v <= prev {
                    return Err(MembershipError);
                }
                prev = v;
                c.insert(v);
            }
            configs.push(c);
        }
        let n = r.byte()?;
        if !(1..=16).contains(&n) {
            return Err(MembershipError);
        }
        let mut nodes = BTreeMap::new();
        let mut prev = 0;
        for _ in 0..n {
            let v = r.u64()?;
            if v <= prev {
                return Err(MembershipError);
            }
            prev = v;
            let len = r.byte()? as usize;
            if !(1..=200).contains(&len) {
                return Err(MembershipError);
            }
            let a = std::str::from_utf8(r.take(len)?).map_err(|_| MembershipError)?;
            if !valid_address(a) {
                return Err(MembershipError);
            }
            nodes.insert(v, a.to_owned());
        }
        let m = Self::new(configs, nodes)?;
        if !r.0.is_empty() || m.encode()? != b {
            return Err(MembershipError);
        }
        Ok(m)
    }
}

struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], MembershipError> {
        let (head, tail) = self.0.split_at_checked(n).ok_or(MembershipError)?;
        self.0 = tail;
        Ok(head)
    }
    fn byte(&mut self) -> Result<u8, MembershipError> {
        Ok(self.take(1)?[0])
    }
    fn u64(&mut self) -> Result<u64, MembershipError> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().map_err(|_| MembershipError)?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const U: &str = "0101010000000000000001020000000000000001016100000000000000020162";
    const J: &str = "0102010000000000000001020000000000000001000000000000000203000000000000000101610000000000000002016200000000000000030163";
    const D: &str = "01020100000000000000010100000000000000010100000000000000010161";
    fn hex(s: &str) -> Vec<u8> {
        s.as_bytes()
            .chunks_exact(2)
            .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
            .collect()
    }
    fn configs(c: &[&[u64]]) -> Vec<BTreeSet<u64>> {
        c.iter().map(|s| s.iter().copied().collect()).collect()
    }
    fn nodes(n: &[(u64, &str)]) -> BTreeMap<u64, String> {
        n.iter().map(|(i, a)| (*i, (*a).to_owned())).collect()
    }
    #[test]
    fn literal_u_encode_preserves_learner() {
        let m = Membership::new(configs(&[&[1]]), nodes(&[(1, "a"), (2, "b")])).unwrap();
        assert_eq!(m.encode().unwrap(), hex(U), "literal U encode equality");
        let decoded = Membership::decode(&hex(U)).unwrap();
        assert_eq!(decoded.configs(), &configs(&[&[1]]));
        assert_eq!(decoded.nodes(), &nodes(&[(1, "a"), (2, "b")]));
        assert_eq!(decoded, m);
    }
    #[test]
    fn constructor_preserves_identical_configs() {
        let expected = configs(&[&[1], &[1]]);
        let m = Membership::new(expected.clone(), nodes(&[(1, "a")])).unwrap();
        assert_eq!(
            m.configs(),
            &expected,
            "direct constructor duplicate configs"
        );
        assert_eq!(m.encode().unwrap(), hex(D));
        assert_eq!(Membership::decode(&hex(D)).unwrap(), m);
    }
    #[test]
    fn constructor_refuses_well_formed_invalid_character() {
        assert_eq!(
            Membership::new(configs(&[&[1]]), nodes(&[(1, "!")])),
            Err(MembershipError),
            "direct constructor invalid character"
        );
    }
    #[test]
    fn literal_joint_and_ordered_domain() {
        let c = configs(&[&[1], &[1, 2]]);
        let n = nodes(&[(1, "a"), (2, "b"), (3, "c")]);
        let m = Membership::new(c.clone(), n.clone()).unwrap();
        assert_eq!(m.encode().unwrap(), hex(J));
        assert_eq!(Membership::decode(&hex(J)).unwrap().into_parts(), (c, n));
        for c in [configs(&[&[2], &[1]]), configs(&[&[1, 2], &[2]])] {
            let n = nodes(&[(1, "localhost"), (2, "[::]-._:Z9")]);
            let m = Membership::new(c.clone(), n.clone()).unwrap();
            assert_eq!(
                Membership::decode(&m.encode().unwrap())
                    .unwrap()
                    .into_parts(),
                (c, n)
            );
        }
        let m = Membership::new(configs(&[&[u64::MAX]]), nodes(&[(u64::MAX, "mem:1")])).unwrap();
        assert_eq!(Membership::decode(&m.encode().unwrap()).unwrap(), m);
    }
    #[test]
    fn maximal_and_every_truncation() {
        let c: BTreeSet<_> = (1..=16).collect();
        let n = (1..=16).map(|i| (i, "a".repeat(200))).collect();
        let m = Membership::new(vec![c.clone(), c], n).unwrap();
        let b = m.encode().unwrap();
        assert_eq!(b.len(), 3605);
        assert_eq!(Membership::decode(&b).unwrap(), m);
        for bytes in [b, hex(U), hex(J), hex(D)] {
            for end in 0..bytes.len() {
                assert_eq!(
                    Membership::decode(&bytes[..end]),
                    Err(MembershipError),
                    "prefix {end}"
                );
            }
        }
    }
    #[test]
    fn constructor_domain_limits() {
        let n = nodes(&[(1, "a")]);
        for c in [
            vec![],
            configs(&[&[1], &[1], &[1]]),
            configs(&[&[]]),
            configs(&[&[0]]),
            configs(&[&[2]]),
            vec![(1..=17).collect()],
        ] {
            assert_eq!(Membership::new(c, n.clone()), Err(MembershipError));
        }
        for n in [
            BTreeMap::new(),
            (1..=17).map(|i| (i, "a".to_owned())).collect(),
            nodes(&[(0, "a"), (1, "a")]),
        ] {
            assert_eq!(Membership::new(configs(&[&[1]]), n), Err(MembershipError));
        }
        for a in [
            "".to_owned(),
            "a".repeat(201),
            "é".to_owned(),
            " ".to_owned(),
            "/".to_owned(),
            "!".to_owned(),
        ] {
            assert_eq!(
                Membership::new(configs(&[&[1]]), [(1, a)].into()),
                Err(MembershipError)
            );
        }
        for a in [
            "a".to_owned(),
            "a".repeat(200),
            "localhost".to_owned(),
            "mem:1".to_owned(),
        ] {
            assert!(Membership::new(configs(&[&[1]]), [(1, a)].into()).is_ok());
        }
    }
    #[test]
    fn well_formed_bad_wire_fields() {
        let u = hex(U);
        let j = hex(J);
        let mut bad = Vec::new();
        for (offset, values) in [
            (0, vec![0, 2]),
            (1, vec![0, 3]),
            (2, vec![0, 17]),
            (11, vec![0, 17]),
            (10, vec![0, 3]),
            (19, vec![0]),
            (29, vec![0, 1]),
        ] {
            for v in values {
                let mut b = u.clone();
                b[offset] = v;
                bad.push(b);
            }
        }
        // Joint voter IDs duplicate or descend, with otherwise complete payloads.
        for id in [0u64, 1] {
            let mut b = j.clone();
            b[20..28].copy_from_slice(&id.to_be_bytes());
            bad.push(b);
        }
        let mut b = j.clone();
        b[12..20].copy_from_slice(&2u64.to_be_bytes());
        b[20..28].copy_from_slice(&1u64.to_be_bytes());
        bad.push(b);
        // Complete but duplicate/descending node table, and missing voter metadata.
        let mut b = u.clone();
        b[22..30].copy_from_slice(&1u64.to_be_bytes());
        bad.push(b);
        let mut b = u.clone();
        b[12..20].copy_from_slice(&3u64.to_be_bytes());
        bad.push(b);
        let mut b = u.clone();
        b[12..20].copy_from_slice(&0u64.to_be_bytes());
        bad.push(b);
        for payload in [
            vec![],
            vec![b'a'; 201],
            vec![255],
            vec![b' '],
            vec![b'/'],
            vec![b'!'],
        ] {
            let mut b = u[..20].to_vec();
            b.push(payload.len() as u8);
            b.extend(payload);
            b.extend(&u[22..]);
            bad.push(b);
        }
        let mut b = u.clone();
        b.push(0);
        bad.push(b);
        let mut b = u.clone();
        b.resize(4096, 0);
        bad.push(b);
        bad.push(vec![0; 4097]);
        for (i, b) in bad.iter().enumerate() {
            assert_eq!(Membership::decode(b), Err(MembershipError), "bad case {i}");
        }
    }
}
