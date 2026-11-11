package net.stamp;

import net.minecraft.world.item.ItemStack;
import net.minecraft.world.level.block.entity.BlockEntity;

/** What a block entity over a tray is asked through: the tray calls what it was given. */
public class ProcessBehaviour {
    public static final Object TYPE = new Object();

    protected final BlockEntity owner;
    private Callback onItem;

    public ProcessBehaviour(BlockEntity owner) {
        this.owner = owner;
        onItem = stack -> false;
    }

    public ProcessBehaviour whenItemRests(Callback callback) {
        onItem = callback;
        return this;
    }

    /** True while it holds the item for itself. */
    public boolean handleItem(ItemStack stack) {
        return onItem.apply(stack);
    }

    public Object getType() {
        return TYPE;
    }

    @FunctionalInterface
    public interface Callback {
        boolean apply(ItemStack stack);
    }
}
