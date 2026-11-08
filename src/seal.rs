//! Reed-Solomon stream codec: splits a stream into `n` equal fragments, any `data` of which
//! rebuild it byte for byte.

use std::collections::BTreeMap;

/// Returns `(data, parity)` for a cluster of `n` nodes.
pub fn scheme(n: usize) -> (usize, usize) {
    let parity = (n - 1) / 2;
    (n - parity, parity)
}

/// One checksummed piece of an encoded stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fragment {
    pub index: usize,
    pub bytes: Vec<u8>,
    pub checksum: [u8; 32],
}

#[derive(Debug, thiserror::Error)]
pub enum SealError {
    #[error("need {needed} valid fragments, have {valid}")]
    TooFewFragments { needed: usize, valid: usize },
    #[error("codec: {0}")]
    Codec(String),
}

impl From<reed_solomon_simd::Error> for SealError {
    fn from(e: reed_solomon_simd::Error) -> Self {
        SealError::Codec(e.to_string())
    }
}

/// BLAKE3 over a domain tag, the stream id, the fragment index, the stream length and the bytes,
/// so a fragment only checks out for the stream, slot and length it was made for.
pub fn checksum(stream: u64, index: usize, len: u64, bytes: &[u8]) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(b"rs3 seal fragment v1");
    h.update(&stream.to_le_bytes());
    h.update(&(index as u64).to_le_bytes());
    h.update(&len.to_le_bytes());
    h.update(bytes);
    *h.finalize().as_bytes()
}

/// Splits the bytes of goal `id`'s stream into `n` equal fragments; any `scheme(n).0` of them
/// rebuild it.
pub fn encode(id: u64, stream: &[u8], n: usize) -> Result<Vec<Fragment>, SealError> {
    let (data, parity) = scheme(n);
    let mut payload = (stream.len() as u64).to_le_bytes().to_vec();
    payload.extend_from_slice(stream);
    payload.resize(payload.len().next_multiple_of(2 * data), 0);
    let originals: Vec<&[u8]> = payload.chunks(payload.len() / data).collect();
    let recovery = reed_solomon_simd::encode(data, parity, &originals)?;
    let shards = originals.into_iter().map(<[u8]>::to_vec).chain(recovery);
    let len = stream.len() as u64;
    Ok(shards
        .enumerate()
        .map(|(index, bytes)| Fragment {
            index,
            checksum: checksum(id, index, len, &bytes),
            bytes,
        })
        .collect())
}

/// Keeps one fragment per index below `total` whose checksum matches stream `id` and `len`.
fn valid(id: u64, len: u64, total: usize, fragments: &[Fragment]) -> BTreeMap<usize, &[u8]> {
    let mut kept = BTreeMap::new();
    for f in fragments {
        if f.index < total && checksum(id, f.index, len, &f.bytes) == f.checksum {
            kept.entry(f.index).or_insert(f.bytes.as_slice());
        }
    }
    kept
}

/// Rebuilds goal `id`'s stream of `len` bytes from fragments of the `(data, parity)` scheme,
/// skipping fragments whose checksum does not match that stream, their index and `len`.
pub fn decode(
    id: u64,
    len: u64,
    data: usize,
    parity: usize,
    fragments: &[Fragment],
) -> Result<Vec<u8>, SealError> {
    let kept = valid(id, len, data + parity, fragments);
    if kept.len() < data {
        return Err(SealError::TooFewFragments {
            needed: data,
            valid: kept.len(),
        });
    }
    let mut shards: BTreeMap<usize, Vec<u8>> = BTreeMap::new();
    let originals = kept
        .iter()
        .filter(|(i, _)| **i < data)
        .map(|(i, b)| (*i, *b));
    shards.extend(originals.map(|(i, b)| (i, b.to_vec())));
    let recovery = kept
        .iter()
        .filter(|(i, _)| **i >= data)
        .map(|(i, b)| (*i - data, *b));
    let originals = shards.iter().map(|(i, b)| (*i, b.as_slice()));
    let restored = reed_solomon_simd::decode(data, parity, originals, recovery)?;
    shards.extend(restored);
    let payload: Vec<u8> = shards.into_values().flatten().collect();
    let end = usize::try_from(len)
        .ok()
        .and_then(|l| l.checked_add(8))
        .filter(|end| *end <= payload.len())
        .ok_or_else(|| SealError::Codec("payload shorter than the stream length".into()))?;
    Ok(payload[8..end].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: u64 = 7;

    fn stream(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i * 31 + i / 7) as u8).collect()
    }

    fn subsets(n: usize, k: usize) -> Vec<Vec<usize>> {
        (0u32..1 << n)
            .filter(|m| m.count_ones() as usize == k)
            .map(|m| (0..n).filter(|i| m >> i & 1 == 1).collect())
            .collect()
    }

    fn pick(all: &[Fragment], idx: &[usize]) -> Vec<Fragment> {
        idx.iter().map(|&i| all[i].clone()).collect()
    }

    #[test]
    fn scheme_splits_nodes_into_data_and_parity() {
        assert_eq!(scheme(5), (3, 2));
        assert_eq!(scheme(3), (2, 1));
        assert_eq!(scheme(4), (3, 1));
    }

    #[test]
    fn any_data_fragments_rebuild_the_stream() {
        for n in [5, 3] {
            let (data, parity) = scheme(n);
            for len in [0, 1, 7, 2 * data * 5 + 1, 2 * data * 5, 100_000] {
                let s = stream(len);
                let frags = encode(ID, &s, n).unwrap();
                assert_eq!(frags.len(), n);
                let size = frags[0].bytes.len();
                assert!(frags.iter().all(|f| f.bytes.len() == size && size > 0));
                assert!(size <= len / data + 8 + 2 * data);
                for sub in subsets(n, data) {
                    let got = decode(ID, len as u64, data, parity, &pick(&frags, &sub)).unwrap();
                    assert_eq!(got, s, "n={n} len={len} subset={sub:?}");
                }
            }
        }
    }

    #[test]
    fn all_fragments_present_rebuild_the_stream() {
        let s = stream(100);
        let frags = encode(ID, &s, 5).unwrap();
        assert_eq!(decode(ID, 100, 3, 2, &frags).unwrap(), s);
    }

    #[test]
    fn corrupt_fragment_is_skipped_while_enough_others_remain() {
        let s = stream(1000);
        let mut frags = encode(ID, &s, 5).unwrap();
        frags[0].bytes[3] ^= 1;
        assert_eq!(decode(ID, 1000, 3, 2, &frags).unwrap(), s);
        let err = decode(ID, 1000, 3, 2, &frags[..3]).unwrap_err();
        assert!(matches!(
            err,
            SealError::TooFewFragments {
                needed: 3,
                valid: 2
            }
        ));
    }

    #[test]
    fn duplicates_and_too_few_fragments_fail() {
        let frags = encode(ID, &stream(50), 5).unwrap();
        let dup = vec![frags[0].clone(), frags[0].clone(), frags[0].clone()];
        assert!(matches!(
            decode(ID, 50, 3, 2, &dup),
            Err(SealError::TooFewFragments {
                needed: 3,
                valid: 1
            })
        ));
        assert!(decode(ID, 50, 3, 2, &frags[..2]).is_err());
    }

    #[test]
    fn fragment_beyond_the_scheme_is_ignored() {
        let frags = encode(ID, &stream(50), 5).unwrap();
        let mut extra = frags[0].clone();
        extra.index = 5;
        let got = vec![frags[1].clone(), frags[2].clone(), extra];
        assert!(matches!(
            decode(ID, 50, 3, 2, &got),
            Err(SealError::TooFewFragments {
                needed: 3,
                valid: 2
            })
        ));
    }

    #[test]
    fn fragment_with_a_valid_checksum_but_an_index_beyond_the_scheme_is_never_used() {
        let frags = encode(ID, &stream(50), 5).unwrap();
        let bytes = frags[0].bytes.clone();
        let extra = Fragment {
            index: 5,
            checksum: checksum(ID, 5, 50, &bytes),
            bytes,
        };
        let got = vec![frags[1].clone(), frags[2].clone(), extra];
        assert!(matches!(
            decode(ID, 50, 3, 2, &got),
            Err(SealError::TooFewFragments {
                needed: 3,
                valid: 2
            })
        ));
    }

    #[test]
    fn decoded_length_is_exactly_the_requested_length() {
        let s = stream(13);
        let frags = encode(ID, &s, 3).unwrap();
        assert_eq!(decode(ID, 13, 2, 1, &frags).unwrap().len(), 13);
        assert!(decode(ID, 10_000, 2, 1, &frags).is_err());
    }

    #[test]
    fn fragments_with_swapped_indexes_are_rejected() {
        let s = stream(300);
        let mut frags = encode(ID, &s, 5).unwrap();
        let (a, b) = (frags[0].index, frags[1].index);
        frags[0].index = b;
        frags[1].index = a;
        let got = pick(&frags, &[0, 1, 2]);
        assert!(matches!(
            decode(ID, 300, 3, 2, &got),
            Err(SealError::TooFewFragments {
                needed: 3,
                valid: 1
            })
        ));
    }

    #[test]
    fn fragments_of_another_stream_of_equal_size_are_rejected() {
        let mine = encode(ID, &stream(300), 5).unwrap();
        let other = encode(ID + 1, &stream(300), 5).unwrap();
        let mixed = [other[0].clone(), mine[1].clone(), mine[2].clone()];
        assert!(decode(ID, 300, 3, 2, &mixed).is_err());
        assert!(decode(ID, 300, 3, 2, &other).is_err());
    }

    #[test]
    fn a_length_other_than_the_encoded_one_never_yields_output() {
        let frags = encode(ID, &stream(300), 5).unwrap();
        for len in [0, 299, 301] {
            assert!(decode(ID, len, 3, 2, &frags).is_err(), "len={len}");
        }
    }
}
