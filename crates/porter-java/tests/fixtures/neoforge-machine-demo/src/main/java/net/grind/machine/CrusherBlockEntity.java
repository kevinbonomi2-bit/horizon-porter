package net.grind.machine;

import net.grind.Grind;
import net.grind.recipe.CrushingRecipe;
import net.minecraft.core.BlockPos;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.network.chat.Component;
import net.minecraft.world.MenuProvider;
import net.minecraft.world.entity.player.Inventory;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.inventory.AbstractContainerMenu;
import net.minecraft.world.inventory.ContainerData;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.state.BlockState;
import net.neoforged.neoforge.fluids.FluidType;
import net.neoforged.neoforge.fluids.capability.templates.FluidTank;
import net.neoforged.neoforge.items.ItemStackHandler;
import net.neoforged.neoforge.transfer.energy.SimpleEnergyHandler;

public class CrusherBlockEntity extends BlockEntity implements MenuProvider {
    public static final int INPUT = 0;
    public static final int OUTPUT = 1;
    public final ItemStackHandler items = new ItemStackHandler(2);
    /** Crushing runs on power: 20 FE a tick. */
    public final SimpleEnergyHandler energy = new SimpleEnergyHandler(10_000, 100, 0);
    /** Water to keep the dust down. */
    public final FluidTank water = new FluidTank(FluidType.BUCKET_VOLUME * 4);
    private int progress;
    private int maxProgress = 100;

    protected final ContainerData data = new ContainerData() {
        @Override
        public int get(int index) {
            return switch (index) {
                case 0 -> progress;
                case 1 -> maxProgress;
                default -> 0;
            };
        }

        @Override
        public void set(int index, int value) {
            switch (index) {
                case 0 -> progress = value;
                case 1 -> maxProgress = value;
            }
        }

        @Override
        public int getCount() {
            return 2;
        }
    };

    public CrusherBlockEntity(BlockPos pos, BlockState state) {
        super(Grind.CRUSHER_BE.get(), pos, state);
    }

    public void tick() {
        ItemStack input = items.getStackInSlot(INPUT);
        CrushingRecipe recipe = CrushingRecipe.find(level, input);
        if (recipe == null) {
            progress = 0;
            return;
        }
        maxProgress = recipe.time();
        progress++;
        if (progress >= maxProgress) {
            ItemStack out = items.getStackInSlot(OUTPUT);
            ItemStack result = recipe.result().copy();
            if (out.isEmpty() || (ItemStack.isSameItem(out, result) && out.getCount() + result.getCount() <= out.getMaxStackSize())) {
                items.extractItem(INPUT, 1, false);
                items.insertItem(OUTPUT, result, false);
            }
            progress = 0;
        }
    }

    @Override
    public Component getDisplayName() {
        return Component.translatable("block.grind.crusher");
    }

    @Override
    public AbstractContainerMenu createMenu(int id, Inventory inventory, Player player) {
        return new CrusherMenu(id, inventory, this, data);
    }

    protected void saveAdditional(CompoundTag tag) {
        tag.putInt("Progress", progress);
        tag.putInt("MaxProgress", maxProgress);
    }

    protected void loadAdditional(CompoundTag tag) {
        progress = tag.getInt("Progress");
        maxProgress = tag.getInt("MaxProgress");
    }
}
