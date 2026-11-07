package net.shutters;

import net.minecraft.core.BlockPos;
import net.minecraft.world.InteractionResult;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.context.UseOnContext;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.BlockStateProperties;

/** Turns the shutter it is used on a quarter turn, wearing a little. */
public class Wrench extends Item {
    public Wrench(Item.Properties properties) {
        super(properties);
    }

    @Override
    public InteractionResult useOn(UseOnContext context) {
        Level level = context.getLevel();
        BlockPos pos = context.getClickedPos();
        BlockState state = level.getBlockState(pos);
        if (!(state.getBlock() instanceof Shutter)) {
            return InteractionResult.PASS;
        }
        level.setBlock(pos, state.setValue(BlockStateProperties.HORIZONTAL_FACING,
            state.getValue(BlockStateProperties.HORIZONTAL_FACING).getClockWise()), 3);
        context.getItemInHand().hurtAndBreak(1, context.getPlayer(), context.getHand());
        return InteractionResult.SUCCESS;
    }
}
