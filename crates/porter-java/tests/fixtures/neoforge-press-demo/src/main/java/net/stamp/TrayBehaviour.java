package net.stamp;

import net.minecraft.core.BlockPos;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.entity.BlockEntity;

/** The item of a tray, and its asking of the block entity two above what to do with it. */
public class TrayBehaviour {
    private final BlockEntity owner;
    public Rested held = new Rested();
    public int cooldown;

    public TrayBehaviour(BlockEntity owner) {
        this.owner = owner;
    }

    public void tick() {
        Level level = owner.getLevel();
        BlockPos pos = owner.getBlockPos();
        if (held.stack.isEmpty()) {
            return;
        }
        ProcessBehaviour above = Behaviours.get(level, pos.above(2), ProcessBehaviour.TYPE);
        if (above == null) {
            return;
        }
        held.locked = above.handleItem(held.stack);
    }

    public void write(CompoundTag tag) {
        tag.putInt("Cooldown", cooldown);
    }

    public void read(CompoundTag tag) {
        cooldown = tag.getInt("Cooldown");
    }
}
