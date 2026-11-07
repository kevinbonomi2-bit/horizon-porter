package com.demo.block;

import com.demo.Demo;
import net.minecraft.core.BlockPos;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.state.BlockState;

public class CrankBlockEntity extends BlockEntity {
    public static final float MAX_SPEED = 256;
    private float speed;
    private int turns;
    private boolean powered;
    private String owner = "";

    public CrankBlockEntity(BlockPos pos, BlockState state) {
        super(Demo.CRANK_BE.get(), pos, state);
    }

    public void wind() {
        speed = Math.min(speed + 16, MAX_SPEED);
        setChanged();
    }

    public void tick() {
        speed = Math.min(speed + 1, MAX_SPEED);
        if (speed >= MAX_SPEED) {
            turns++;
        }
    }

    protected void saveAdditional(CompoundTag tag) {
        tag.putFloat("Speed", speed);
        tag.putInt("Turns", turns);
        tag.putBoolean("Powered", powered);
        tag.putString("Owner", owner);
    }

    protected void loadAdditional(CompoundTag tag) {
        speed = tag.getFloat("Speed");
        turns = tag.getInt("Turns");
        powered = tag.getBoolean("Powered");
        owner = tag.getString("Owner");
    }
}
