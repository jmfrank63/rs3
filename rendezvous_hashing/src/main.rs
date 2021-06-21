use std::collections::HashMap;
use rendezvous_hash::{RendezvousNodes, Node};
use rendezvous_hash::{WeightedNode, Capacity};

fn main() {
    let mut nodes = RendezvousNodes::default();
    nodes.insert(WeightedNode::new("foo", Capacity::new(70.0).unwrap()));
    nodes.insert(WeightedNode::new("bar", Capacity::new(20.0).unwrap()));
    nodes.insert(WeightedNode::new("baz", Capacity::new(9.0).unwrap()));
    nodes.insert(WeightedNode::new("qux", Capacity::new(1.0).unwrap()));
    println!("{}", nodes.len());

    for node in nodes.iter() {
        println!("{}", node.node_id());
    }

    nodes.remove(&"foo");
    nodes.remove(&"qux");
    println!("{}", nodes.len());
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

        let mut counts = HashMap::new();
        let amount = 10000;
        for item in 0..amount {
            let node = nodes.calc_candidates(&item).nth(0).unwrap();
            *counts.entry(node.node.to_string()).or_insert(0) += 1;
        }
        assert_eq!(((counts["foo"] as f64) / (amount as f64 / 100.0)).round(), 70.0);
        assert_eq!(((counts["bar"] as f64) / (amount as f64 / 100.0)).round(), 20.0);
        assert_eq!(((counts["baz"] as f64) / (amount as f64 / 100.0)).round(), 9.0);
        assert_eq!(((counts["qux"] as f64) / (amount as f64 / 100.0)).round(), 1.0);
    }
}