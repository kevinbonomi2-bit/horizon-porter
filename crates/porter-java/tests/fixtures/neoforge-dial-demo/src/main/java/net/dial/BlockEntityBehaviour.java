package net.dial;

import net.minecraft.nbt.CompoundTag;

/** A part of a block entity that keeps state of its own. */
public abstract class BlockEntityBehaviour {
    public SmartBlockEntity blockEntity;

    public BlockEntityBehaviour(SmartBlockEntity be) {
        blockEntity = be;
    }

    public void write(CompoundTag nbt) {
    }

    public void read(CompoundTag nbt) {
    }
}
