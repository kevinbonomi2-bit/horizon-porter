package net.grind.machine;

import net.grind.Grind;
import net.minecraft.core.BlockPos;
import net.minecraft.core.NonNullList;
import net.minecraft.network.chat.Component;
import net.minecraft.world.entity.player.Inventory;
import net.minecraft.world.inventory.AbstractContainerMenu;
import net.minecraft.world.inventory.ChestMenu;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.level.block.entity.RandomizableContainerBlockEntity;
import net.minecraft.world.level.block.state.BlockState;

public class CabinetBlockEntity extends RandomizableContainerBlockEntity {
    private NonNullList<ItemStack> contents = NonNullList.withSize(27, ItemStack.EMPTY);

    public CabinetBlockEntity(BlockPos pos, BlockState state) {
        super(Grind.CABINET_BE.get(), pos, state);
    }

    @Override
    protected Component getDefaultName() {
        return Component.translatable("container.grind.cabinet");
    }

    @Override
    protected AbstractContainerMenu createMenu(int id, Inventory player) {
        return ChestMenu.threeRows(id, player, this);
    }

    @Override
    public int getContainerSize() {
        return 27;
    }
}
