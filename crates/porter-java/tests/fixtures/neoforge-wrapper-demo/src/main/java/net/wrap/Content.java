package net.wrap;

import java.util.function.Function;
import net.minecraft.core.registries.Registries;
import net.minecraft.world.item.Item;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockBehaviour;
import net.neoforged.neoforge.registries.DeferredHolder;

public class Content {
    static final GameRegistry<Block> BLOCKS = new GameRegistry<>(Registries.BLOCK);
    static final GameRegistry<Item> ITEMS = new GameRegistry<>(Registries.ITEM);

    public static final DeferredHolder<Block, CrateBlock> crate = blockWithItem("crate", CrateBlock::new, p -> p.strength(2.0f));
    public static final DeferredHolder<Item, KeyItem> key = item("key", KeyItem::new);
    public static final DeferredHolder<Block, CableBlock> cable = blockWithItem("cable", CableBlock::new, p -> p.strength(0.5f));
    public static final DeferredHolder<Block, PanelBlock> panel = blockWithItem("panel", PanelBlock::new, p -> p.strength(1.0f));

    private static <B extends Block> DeferredHolder<Block, B> blockWithItem(String name, Function<BlockBehaviour.Properties, B> create, Function<BlockBehaviour.Properties, BlockBehaviour.Properties> props) {
        DeferredHolder<Block, B> block = BLOCKS.register(name, () -> create.apply(props.apply(BlockBehaviour.Properties.of())));
        ITEMS.register(name, () -> new Item(new Item.Properties()));
        return block;
    }

    private static <I extends Item> DeferredHolder<Item, I> item(String name, Function<Item.Properties, I> create) {
        return ITEMS.register(name, () -> create.apply(new Item.Properties()));
    }
}
