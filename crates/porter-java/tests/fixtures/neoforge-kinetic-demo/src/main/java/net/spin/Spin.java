package net.spin;

import com.tterrag.registrate.Registrate;
import com.tterrag.registrate.util.entry.BlockEntityEntry;
import com.tterrag.registrate.util.entry.BlockEntry;
import com.tterrag.registrate.util.entry.ItemEntry;
import net.minecraft.world.item.Item;
import net.minecraft.world.level.block.Blocks;
import net.neoforged.fml.common.Mod;

/** A tiny Create-style mod: a hand crank turns shafts, and turning spreads. */
@Mod(Spin.ID)
public class Spin {
    public static final String ID = "spin";
    public static final Registrate REGISTRATE = Registrate.create(ID);

    public static final BlockEntry<CrankBlock> CRANK = REGISTRATE.block("crank", CrankBlock::new)
        .initialProperties(() -> Blocks.OAK_PLANKS)
        .properties(p -> p.strength(1.5f).noOcclusion())
        .lang("Hand Crank")
        .simpleItem()
        .register();

    public static final BlockEntry<ShaftBlock> SHAFT = REGISTRATE.block("shaft", ShaftBlock::new)
        .initialProperties(() -> Blocks.IRON_BLOCK)
        .properties(p -> p.strength(2.0f).noOcclusion())
        .lang("Shaft")
        .simpleItem()
        .register();

    public static final BlockEntityEntry<KineticBlockEntity> KINETIC = REGISTRATE
        .blockEntity("kinetic", KineticBlockEntity::new)
        .validBlocks(CRANK, SHAFT)
        .renderer(() -> KineticRenderer::new)
        .register();

    public static final ItemEntry<Item> WRENCH = REGISTRATE.item("wrench", Item::new)
        .lang("Wrench")
        .register();
}
