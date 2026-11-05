package net.grind.machine;

import net.grind.Grind;
import net.minecraft.network.RegistryFriendlyByteBuf;
import net.minecraft.world.entity.player.Inventory;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.inventory.AbstractContainerMenu;
import net.minecraft.world.inventory.ContainerData;
import net.minecraft.world.inventory.SimpleContainerData;
import net.minecraft.world.inventory.Slot;
import net.minecraft.world.item.ItemStack;
import net.neoforged.neoforge.items.ItemStackHandler;
import net.neoforged.neoforge.items.SlotItemHandler;

public class CrusherMenu extends AbstractContainerMenu {
    private final ContainerData data;

    public CrusherMenu(int id, Inventory inventory, RegistryFriendlyByteBuf extra) {
        this(id, inventory, new ItemStackHandler(2), new SimpleContainerData(2));
    }

    public CrusherMenu(int id, Inventory inventory, CrusherBlockEntity crusher, ContainerData data) {
        this(id, inventory, crusher.items, data);
    }

    private CrusherMenu(int id, Inventory inventory, ItemStackHandler items, ContainerData data) {
        super(Grind.CRUSHER_MENU.get(), id);
        this.data = data;
        addSlot(new SlotItemHandler(items, 0, 56, 35));
        addSlot(new OutputSlot(items, 1, 116, 35));
        for (int row = 0; row < 3; row++) {
            for (int col = 0; col < 9; col++) {
                addSlot(new Slot(inventory, col + row * 9 + 9, 8 + col * 18, 84 + row * 18));
            }
        }
        for (int col = 0; col < 9; col++) {
            addSlot(new Slot(inventory, col, 8 + col * 18, 142));
        }
        addDataSlots(data);
    }

    public int scaledProgress() {
        int progress = data.get(0);
        int max = data.get(1);
        return max == 0 ? 0 : progress * 24 / max;
    }

    @Override
    public ItemStack quickMoveStack(Player player, int index) {
        return ItemStack.EMPTY;
    }

    @Override
    public boolean stillValid(Player player) {
        return true;
    }

    /** The output: players only take from it. */
    static class OutputSlot extends SlotItemHandler {
        OutputSlot(ItemStackHandler items, int index, int x, int y) {
            super(items, index, x, y);
        }

        @Override
        public boolean mayPlace(ItemStack stack) {
            return false;
        }
    }
}
