package net.dial;

import java.util.ArrayList;
import java.util.List;
import net.minecraft.core.BlockPos;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.entity.BlockEntityType;
import net.minecraft.world.level.block.state.BlockState;

/** A block entity made of behaviours, as Create's are. */
public abstract class SmartBlockEntity extends BlockEntity {
    private final List<BlockEntityBehaviour> behaviours = new ArrayList<>();

    public SmartBlockEntity(BlockEntityType<?> type, BlockPos pos, BlockState state) {
        super(type, pos, state);
        addBehaviours(behaviours);
    }

    public abstract void addBehaviours(List<BlockEntityBehaviour> behaviours);

    @Override
    protected void saveAdditional(CompoundTag tag, net.minecraft.core.HolderLookup.Provider registries) {
        for (BlockEntityBehaviour b : behaviours)
            b.write(tag);
    }

    @Override
    protected void loadAdditional(CompoundTag tag, net.minecraft.core.HolderLookup.Provider registries) {
        for (BlockEntityBehaviour b : behaviours)
            b.read(tag);
    }
}
