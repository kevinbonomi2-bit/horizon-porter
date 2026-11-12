package net.stamp;

import net.minecraft.core.BlockPos;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.entity.BlockEntityType;
import net.minecraft.world.level.block.state.BlockState;

/** What the press and the tray share: they age, and are told each second. */
public abstract class StampBlockEntity extends BlockEntity {
    protected int age;

    public StampBlockEntity(BlockEntityType<?> type, BlockPos pos, BlockState state) {
        super(type, pos, state);
    }

    public void tick() {
        age++;
        if (age % 20 == 0) {
            onSecond();
        }
    }

    /** Called each second; a class below does its own work here. */
    protected void onSecond() {}

    @Override
    protected void saveAdditional(CompoundTag tag, net.minecraft.core.HolderLookup.Provider registries) {
        tag.putInt("Age", age);
    }

    @Override
    protected void loadAdditional(CompoundTag tag, net.minecraft.core.HolderLookup.Provider registries) {
        age = tag.getInt("Age");
    }
}
