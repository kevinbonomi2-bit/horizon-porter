package net.spin;

import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.state.BlockState;

/** Speed in RPM: a crank makes 16, a shaft takes the fastest neighbour's. */
public class KineticBlockEntity extends BlockEntity {
    private float speed;

    public KineticBlockEntity(BlockPos pos, BlockState state) {
        super(Spin.KINETIC.get(), pos, state);
    }

    public float getSpeed() {
        return speed;
    }

    public void tick() {
        Level level = getLevel();
        float next = 0;
        if (getBlockState().is(Spin.CRANK.get())) {
            next = 16;
        } else {
            for (Direction side : Direction.values()) {
                if (level.getBlockEntity(getBlockPos().relative(side)) instanceof KineticBlockEntity other) {
                    next = Math.max(next, Math.abs(other.getSpeed()));
                }
            }
        }
        if (next != speed) {
            speed = next;
            setChanged();
            level.sendBlockUpdated(getBlockPos(), getBlockState(), getBlockState(), 3);
        }
    }

    @Override
    protected void saveAdditional(CompoundTag tag, net.minecraft.core.HolderLookup.Provider registries) {
        tag.putFloat("Speed", speed);
    }

    @Override
    protected void loadAdditional(CompoundTag tag, net.minecraft.core.HolderLookup.Provider registries) {
        speed = tag.getFloat("Speed");
    }
}
