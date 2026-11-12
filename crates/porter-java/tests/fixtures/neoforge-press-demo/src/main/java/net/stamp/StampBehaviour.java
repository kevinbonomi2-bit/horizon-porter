package net.stamp;

import net.minecraft.core.Direction;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.state.properties.BlockStateProperties;
import net.stamp.config.StampConfig;

/** The cycle of a press: it starts when an item rests under it and stamps halfway through. */
public class StampBehaviour extends ProcessBehaviour {
    public static final int CYCLE = 40;

    public boolean running;
    public int ticks;

    public StampBehaviour(BlockEntity owner) {
        super(owner);
        whenItemRests(this::onItem);
    }

    private boolean onItem(ItemStack stack) {
        if (!stack.is(Items.IRON_INGOT)) {
            return false;
        }
        // A press turned along x stamps nothing while its head is down.
        boolean alongX = owner.getBlockState().getValue(BlockStateProperties.HORIZONTAL_FACING).getAxis() == Direction.Axis.X;
        if (alongX && owner.getBlockState().getValue(PressBlock.LOWERED)) {
            return false;
        }
        // Only a large one stamps.
        if (owner.getBlockState().getBlock() instanceof ICog cog && !cog.isLarge()) {
            return false;
        }
        if (!running) {
            running = true;
            ticks = 0;
            return true;
        }
        if (ticks == CYCLE / 2) {
            int made = StampConfig.server().recipes.bulk.get() ? stack.getCount() : 1;
            stack.shrink(made);
        }
        return true;
    }

    public void tick() {
        if (!running) {
            return;
        }
        ticks++;
        if (ticks > CYCLE) {
            running = false;
        }
    }

    public void write(CompoundTag tag) {
        tag.putBoolean("Running", running);
        tag.putInt("Ticks", ticks);
    }

    public void read(CompoundTag tag) {
        running = tag.getBoolean("Running");
        ticks = tag.getInt("Ticks");
    }
}
