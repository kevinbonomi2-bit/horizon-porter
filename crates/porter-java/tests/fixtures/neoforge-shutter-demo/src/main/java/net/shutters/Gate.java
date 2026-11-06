package net.shutters;

import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.util.RandomSource;
import net.minecraft.world.level.LevelReader;
import net.minecraft.world.level.ScheduledTickAccess;
import net.minecraft.world.level.block.state.BlockState;

/** A shutter that opens with the gate beside it. */
public class Gate extends Shutter {
    public Gate(Properties properties) {
        super(properties);
    }

    protected BlockState updateShape(BlockState state, LevelReader level, ScheduledTickAccess ticks, BlockPos pos, Direction direction, BlockPos neighborPos, BlockState neighbor, RandomSource random) {
        if (neighbor.getBlock() instanceof Gate && direction.getAxis().isHorizontal()) {
            return state.setValue(OPEN, neighbor.getValue(OPEN));
        }
        return state;
    }
}
