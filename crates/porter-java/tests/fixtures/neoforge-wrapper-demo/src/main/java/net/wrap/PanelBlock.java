package net.wrap;

import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.phys.shapes.VoxelShape;

/** A thin panel that joins cables. */
public class PanelBlock extends Block implements ICable {
    static final VoxelShape SHAPE = Block.box(0, 0, 0, 16, 16, 2);

    public PanelBlock(Properties p) {
        super(p);
    }

    protected VoxelShape getShape(BlockState s, net.minecraft.world.level.BlockGetter l, net.minecraft.core.BlockPos p, net.minecraft.world.phys.shapes.CollisionContext c) {
        return SHAPE;
    }
}
