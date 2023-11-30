type NodeId = shipyard::EntityId;

/// An immutable reference to a node in a RealDom
pub struct NodeRef {
    id: NodeId,
    // dom: &'a RealDom<V>,
}
