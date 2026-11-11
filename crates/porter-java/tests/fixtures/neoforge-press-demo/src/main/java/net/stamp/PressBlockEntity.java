package net.stamp;

import net.minecraft.core.BlockPos;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.state.BlockState;

public class PressBlockEntity extends BlockEntity {
    public StampBehaviour stamping = new StampBehaviour(this);

    public PressBlockEntity(BlockPos pos, BlockState state) {
        super(Stamp.PRESS_BE.get(), pos, state);
    }

    public void tick() {
        stamping.tick();
    }

    @Override
    protected void saveAdditional(CompoundTag tag, net.minecraft.core.HolderLookup.Provider registries) {
        stamping.write(tag);
    }

    @Override
    protected void loadAdditional(CompoundTag tag, net.minecraft.core.HolderLookup.Provider registries) {
        stamping.read(tag);
    }
}
