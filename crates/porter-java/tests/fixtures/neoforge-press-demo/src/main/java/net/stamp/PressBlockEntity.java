package net.stamp;

import net.minecraft.core.BlockPos;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.state.BlockState;

public class PressBlockEntity extends StampBlockEntity {
    public StampBehaviour stamping = new StampBehaviour(this);

    public PressBlockEntity(BlockPos pos, BlockState state) {
        super(Stamp.PRESS_BE.get(), pos, state);
    }

    @Override
    public void tick() {
        super.tick();
        stamping.tick();
    }

    @Override
    protected int pace() {
        return stamping.running ? 2 : 1;
    }

    @Override
    protected void onSecond() {
        if (!stamping.running) {
            stamping.ticks = 0;
        }
    }

    @Override
    protected void saveAdditional(CompoundTag tag, net.minecraft.core.HolderLookup.Provider registries) {
        super.saveAdditional(tag, registries);
        stamping.write(tag);
    }

    @Override
    protected void loadAdditional(CompoundTag tag, net.minecraft.core.HolderLookup.Provider registries) {
        super.loadAdditional(tag, registries);
        stamping.read(tag);
    }
}
