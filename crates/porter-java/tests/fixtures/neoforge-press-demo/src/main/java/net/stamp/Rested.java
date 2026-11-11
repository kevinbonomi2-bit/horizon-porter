package net.stamp;

import net.minecraft.world.item.ItemStack;

/** The stack on a tray, and whether something holds it there. */
public class Rested {
    public ItemStack stack = ItemStack.EMPTY;
    public boolean locked;
}
