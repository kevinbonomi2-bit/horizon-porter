package net.stamp;

import net.minecraft.core.BlockPos;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.state.BlockState;

public class TrayBlockEntity extends StampBlockEntity {
    public TrayBehaviour tray = new TrayBehaviour(this);

    public TrayBlockEntity(BlockPos pos, BlockState state) {
        super(Stamp.TRAY_BE.get(), pos, state);
    }

    @Override
    public void tick() {
        super.tick();
        tray.tick();
    }

    @Override
    protected void saveAdditional(CompoundTag tag, net.minecraft.core.HolderLookup.Provider registries) {
        super.saveAdditional(tag, registries);
        tray.write(tag);
    }

    @Override
    protected void loadAdditional(CompoundTag tag, net.minecraft.core.HolderLookup.Provider registries) {
        super.loadAdditional(tag, registries);
        tray.read(tag);
    }
}
