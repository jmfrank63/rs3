mod shift_xor_hash;

use crate::shift_xor_hash::{BuildShiftXorHasher, ShiftXorHasher};
use rendezvous_hash::{Capacity, DefaultNodeHasher, NodeHasher, WeightedNode};
use rendezvous_hash::{Node, RendezvousNodes};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

impl<N: Hash> NodeHasher<N> for ShiftXorHasher {
    fn hash<T: Hash>(&self, node: &N, item: &T) -> u64 {
        let mut hasher = ShiftXorHasher::default();
        node.hash(&mut hasher);
        item.hash(&mut hasher);
        hasher.finish()
    }
}

fn main() {
    let mut weighted_nodes = RendezvousNodes::default();
    // nodes.insert(WeightedNode::new("foo", Capacity::new(70.0).unwrap()));
    weighted_nodes.insert(WeightedNode::new("bar", Capacity::new(20.0).unwrap()));
    weighted_nodes.insert(WeightedNode::new("baz", Capacity::new(9.0).unwrap()));
    weighted_nodes.insert(WeightedNode::new("qux", Capacity::new(1.0).unwrap()));
    println!("{}", weighted_nodes.len());

    for node in weighted_nodes.iter() {
        println!("{}", node.node_id());
    }

    weighted_nodes.remove(&"foo");
    weighted_nodes.remove(&"qux");
    println!("{}", weighted_nodes.len());

    let mut counts = HashMap::new();
    let amount = 10000;
    add_map_entries(&mut weighted_nodes, &mut counts, amount);
    for key in counts.iter() {
        println!("{}", counts[key.0]);
    }

    let mut hash_map = HashMap::with_hasher(BuildShiftXorHasher);

    hash_map.insert("foo", "some id");

    let mut custom_nodes: RendezvousNodes<&str, ShiftXorHasher> =
        RendezvousNodes::new(ShiftXorHasher::default());

    custom_nodes.insert("foo");
}

fn add_map_entries(
    weighted_nodes: &mut RendezvousNodes<WeightedNode<&str>, DefaultNodeHasher>,
    counts: &mut HashMap<String, u32>,
    amount: u32,
) {
    for item in 0..amount {
        let node = weighted_nodes.calc_candidates(&item).nth(0).unwrap();
        *counts.entry(node.node.to_string()).or_insert(0) += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distribution_is_according_to_capacity() {
        let mut nodes = RendezvousNodes::default();
        nodes.insert(WeightedNode::new("foo", Capacity::new(70.0).unwrap()));
        nodes.insert(WeightedNode::new("bar", Capacity::new(20.0).unwrap()));
        nodes.insert(WeightedNode::new("baz", Capacity::new(9.0).unwrap()));
        nodes.insert(WeightedNode::new("qux", Capacity::new(1.0).unwrap()));

        let mut counts: HashMap<String, u32> = HashMap::new();
        let amount: u32 = 10000;
        add_map_entries(&mut weighted_nodes, &mut counts, amount);
        assert_eq!(
            ((counts["foo"] as f64) / (amount as f64 / 100.0)).round(),
            70.0
        );
        assert_eq!(
            ((counts["bar"] as f64) / (amount as f64 / 100.0)).round(),
            20.0
        );
        assert_eq!(
            ((counts["baz"] as f64) / (amount as f64 / 100.0)).round(),
            9.0
        );
        assert_eq!(
            ((counts["qux"] as f64) / (amount as f64 / 100.0)).round(),
            1.0
        );
    }
}
