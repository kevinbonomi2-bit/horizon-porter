# modkit

The runtime layer that ported mods build on: a Fabric/NeoForge-like API (registries,
block entities, events, networking, menus, config, rendering) on top of Horizon's hook
points, packaged as a [horizon-mixins](https://github.com/horizon-rust/horizon-mixins)
bundle with its own crates.

It is not a Cargo workspace member here, because it is compiled *inside* Horizon's
source by horizon-mixins, not on its own. See [docs/ARCHITECTURE.md](../docs/ARCHITECTURE.md#modkit-the-runtime-layer).

Nothing is here yet: modkit starts at milestone M2 in [the roadmap](../docs/ROADMAP.md).
