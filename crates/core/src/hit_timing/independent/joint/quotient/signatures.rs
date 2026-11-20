//! Canonical continuation keys. Most nodes have at most two continuations;
//! keeping those inline avoids a separate allocation for each temporary key.
use std::hash::{Hash, Hasher};

pub(super) type Atom = (usize, u8, usize);
pub(super) trait Key: Default + Clone + Eq + Hash {
    fn add(&mut self, atom: Atom);
    fn normalize(&mut self);
}

impl Key for Vec<Atom> {
    fn add(&mut self, atom: Atom) { self.push(atom); }
    fn normalize(&mut self) { self.sort_unstable(); self.dedup(); }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Inline {
    Small { len: u8, atoms: [u64; 2] },
    Large(Vec<Atom>),
}
const DESTINATION_MASK: u64 = (1 << 30) - 1;
fn pack((action, kind, destination): Atom) -> Option<u64> {
    if kind > 2 || destination as u64 > DESTINATION_MASK { return None; }
    let action = if action == usize::MAX { u32::MAX }
        else { let value = u32::try_from(action).ok()?; if value == u32::MAX { return None; } value };
    Some((u64::from(action) << 32) | (u64::from(kind) << 30) | destination as u64)
}
fn unpack(value: u64) -> Atom {
    let action = (value >> 32) as u32;
    (if action == u32::MAX { usize::MAX } else { action as usize },
        ((value >> 30) & 3) as u8, (value & DESTINATION_MASK) as usize)
}
impl Default for Inline {
    fn default() -> Self { Self::Small { len: 0, atoms: [0; 2] } }
}
impl Hash for Inline {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Self::Small { len, atoms } => {
                usize::from(*len).hash(state);
                for &atom in &atoms[..usize::from(*len)] { unpack(atom).hash(state); }
            },
            Self::Large(values) => values.hash(state),
        }
    }
}
impl Key for Inline {
    fn add(&mut self, atom: Atom) {
        match self {
            Self::Small { len, atoms } => {
                if let Some(value) = pack(atom) {
                    if atoms[..usize::from(*len)].contains(&value) { return; }
                    if *len < 2 {
                        atoms[usize::from(*len)] = value;
                        *len += 1;
                        return;
                    }
                }
                let mut values = Vec::with_capacity(4);
                values.extend(atoms[..usize::from(*len)].iter().map(|&value| unpack(value)));
                values.push(atom);
                *self = Self::Large(values);
            }
            Self::Large(values) => values.push(atom),
        }
    }
    fn normalize(&mut self) {
        match self {
            Self::Small { len, atoms } => {
                if *len == 2 {
                    if atoms[1] < atoms[0] { atoms.swap(0, 1); }
                    if atoms[0] == atoms[1] {
                        *len = 1;
                        atoms[1] = 0;
                    }
                }
            }
            Self::Large(values) => {
                values.sort_unstable();
                values.dedup();
                if values.len() <= 2 {
                    let mut atoms = [0; 2];
                    for (slot, &atom) in atoms.iter_mut().zip(values.iter()) {
                        let Some(value) = pack(atom) else { return; };
                        *slot = value;
                    }
                    *self = Self::Small { len: values.len() as u8, atoms };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    #[test]
    fn packed_atoms_cover_exact_limits_and_keep_unrepresentable_values_in_vectors() {
        assert!(std::mem::size_of::<Inline>() <= 32);
        for action in [0, 1, u32::MAX as usize - 1, u32::MAX as usize, usize::MAX] {
            for kind in [0, 1, 2, 3, u8::MAX] {
                for destination in [0, 1, DESTINATION_MASK as usize,
                    DESTINATION_MASK as usize + 1, usize::MAX] {
                    let atom = (action, kind, destination);
                    if let Some(value) = pack(atom) { assert_eq!(unpack(value), atom); }
                    let mut key = Inline::default(); key.add(atom); key.normalize();
                    let mut a = std::collections::hash_map::DefaultHasher::new();
                    let mut b = std::collections::hash_map::DefaultHasher::new();
                    key.hash(&mut a); vec![atom].hash(&mut b);
                    assert_eq!(a.finish(), b.finish());
                    if pack(atom).is_none() { assert!(matches!(key, Inline::Large(_))); }
                }
            }
        }
    }
    #[test]
    fn inline_keys_preserve_independent_vector_classes_and_normalize_unused_cells() {
        let alphabet = [(0, 0, 0), (usize::MAX, 2, 0), (1, 1, usize::MAX)];
        let mut inline_classes = HashMap::new();
        let mut vector_classes = HashMap::new();
        for len in 0..=7 {
            for pattern in 0..3usize.pow(len) {
                let mut key = Inline::default();
                let mut expected = Vec::new();
                for position in 0..len {
                    let atom = alphabet[pattern / 3usize.pow(position) % 3];
                    key.add(atom);
                    expected.push(atom);
                }
                key.normalize();
                expected.sort_unstable();
                expected.dedup();
                let mut canonical = Inline::default();
                for &atom in &expected { canonical.add(atom); }
                canonical.normalize();
                assert_eq!(key, canonical);
                let mut actual_hash = std::collections::hash_map::DefaultHasher::new();
                let mut expected_hash = std::collections::hash_map::DefaultHasher::new();
                key.hash(&mut actual_hash);
                expected.hash(&mut expected_hash);
                assert_eq!(actual_hash.finish(), expected_hash.finish(), "{key:?}");
                let next_inline = inline_classes.len();
                let actual = *inline_classes.entry(key).or_insert(next_inline);
                let next_vector = vector_classes.len();
                let expected = *vector_classes.entry(expected).or_insert(next_vector);
                assert_eq!(actual, expected);
            }
        }
    }
}
