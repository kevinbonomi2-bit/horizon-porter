package com.demo.block;

import com.demo.Demo;
import net.minecraft.core.BlockPos;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.state.BlockState;

public class CrankBlockEntity extends BlockEntity {
    private float speed;

    public CrankBlockEntity(BlockPos pos, BlockState state) {
        super(Demo.CRANK_BE.get(), pos, state);
    }

    public void tick() {
        speed = Math.min(speed + 1, 256);
    }

    protected void saveAdditional(CompoundTag tag) {
        tag.putFloat("Speed", speed);
    }
}
