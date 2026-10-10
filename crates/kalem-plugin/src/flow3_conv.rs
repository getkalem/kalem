// The `flow-3` interface's types (API 0.2.9) between the Rust contract
// (`kv`) and the WIT bindings (`f3`), both ways. Included after
// `annotations_conv.rs` (its `Cross`) by `kalem-plugin`'s adapter and by
// `kalem-script`'s host.

impl Cross<f3::FlowChange> for kv::FlowChange {
    fn cross(self) -> f3::FlowChange {
        f3::FlowChange {
            from: self.from,
            removed: self.removed,
            added: self.added,
            shift: self.shift,
        }
    }
}

impl Cross<kv::FlowChange> for f3::FlowChange {
    fn cross(self) -> kv::FlowChange {
        kv::FlowChange {
            from: self.from,
            removed: self.removed,
            added: self.added,
            shift: self.shift,
        }
    }
}
