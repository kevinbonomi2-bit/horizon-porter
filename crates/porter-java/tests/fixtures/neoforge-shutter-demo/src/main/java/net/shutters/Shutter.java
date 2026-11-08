package net.shutters;

import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.sounds.SoundEvents;
import net.minecraft.sounds.SoundSource;
import net.minecraft.world.InteractionResult;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.item.context.BlockPlaceContext;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.HorizontalDirectionalBlock;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.StateDefinition.Builder;
import net.minecraft.world.level.block.state.properties.BlockStateProperties;
import net.minecraft.world.level.block.state.properties.BooleanProperty;
import net.minecraft.world.level.block.state.properties.DoorHingeSide;
import net.minecraft.world.level.block.state.properties.EnumProperty;
import net.minecraft.world.phys.BlockHitResult;

/** One block tall, hinged on the side clicked, opened and closed by hand. */
public class Shutter extends Block {
    public static final EnumProperty<Direction> FACING = HorizontalDirectionalBlock.FACING;
    public static final BooleanProperty OPEN = BlockStateProperties.OPEN;
    public static final EnumProperty<DoorHingeSide> HINGE = BlockStateProperties.DOOR_HINGE;

    public Shutter(Properties properties) {
        super(properties);
    }

    public BlockState getStateForPlacement(BlockPlaceContext context) {
        double x = context.getClickLocation().x - context.getClickedPos().getX();
        DoorHingeSide hinge = x < 0.5 ? DoorHingeSide.LEFT : DoorHingeSide.RIGHT;
        return this.defaultBlockState().setValue(HINGE, hinge).setValue(FACING, context.getHorizontalDirection().getClockWise());
    }

    protected InteractionResult useWithoutItem(BlockState state, Level level, BlockPos pos, Player player, BlockHitResult hit) {
        state = state.cycle(OPEN);
        level.setBlock(pos, state, 2);
        level.playSound(null, pos, SoundEvents.WOODEN_TRAPDOOR_OPEN, SoundSource.BLOCKS, 1.0F, 1.0F);
        return InteractionResult.SUCCESS;
    }

    protected void randomTick(BlockState state, ServerLevel level, BlockPos pos, RandomSource random) {
        if (random.nextInt(10) == 0) {
            level.setBlock(pos, state.cycle(OPEN), 3);
        }
    }

    protected void tick(BlockState state, ServerLevel level, BlockPos pos, RandomSource random) {
        level.setBlock(pos, state.setValue(OPEN, false), 3);
    }

    protected boolean canSurvive(BlockState state, LevelReader level, BlockPos pos) {
        return !level.getBlockState(pos.below()).isAir();
    }

    protected int getAnalogOutputSignal(BlockState state, Level level, BlockPos pos, Direction direction) {
        return state.getValue(OPEN) ? 15 : 0;
    }

    public void setPlacedBy(Level level, BlockPos pos, BlockState state, LivingEntity placer, ItemStack stack) {
        level.scheduleTick(pos, this, 20);
    }

    public BlockState playerWillDestroy(Level level, BlockPos pos, BlockState state, Player player) {
        level.playSound(null, pos, SoundEvents.WOODEN_TRAPDOOR_CLOSE, SoundSource.BLOCKS, 1.0F, 1.0F);
        return super.playerWillDestroy(level, pos, state, player);
    }

    public void appendHoverText(ItemStack stack, Item.TooltipContext context, TooltipDisplay display, Consumer<Component> tooltip, TooltipFlag flag) {
        tooltip.accept(Component.translatable("tooltip.shutters.shutter").withStyle(ChatFormatting.GRAY));
    }

    protected void createBlockStateDefinition(Builder<Block, BlockState> builder) {
        builder.add(FACING, OPEN, HINGE);
    }
}
