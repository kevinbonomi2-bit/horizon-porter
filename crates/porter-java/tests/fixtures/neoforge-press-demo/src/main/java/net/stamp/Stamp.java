package net.stamp;

import com.tterrag.registrate.Registrate;
import com.tterrag.registrate.util.entry.BlockEntityEntry;
import com.tterrag.registrate.util.entry.BlockEntry;
import net.minecraft.world.level.block.Blocks;
import net.neoforged.fml.common.Mod;

/** A tiny Create-style mod: a press stamps the item on the tray two blocks under it. */
@Mod(Stamp.ID)
public class Stamp {
    public static final String ID = "stamp";
    public static final Registrate REGISTRATE = Registrate.create(ID);

    public static final BlockEntry<PressBlock> PRESS = REGISTRATE.block("press", PressBlock::new)
        .initialProperties(() -> Blocks.IRON_BLOCK)
        .lang("Press")
        .simpleItem()
        .register();

    public static final BlockEntry<TrayBlock> TRAY = REGISTRATE.block("tray", TrayBlock::new)
        .initialProperties(() -> Blocks.OAK_PLANKS)
        .lang("Tray")
        .simpleItem()
        .register();

    public static final BlockEntityEntry<PressBlockEntity> PRESS_BE = REGISTRATE
        .blockEntity("press", PressBlockEntity::new)
        .validBlocks(PRESS)
        .register();

    public static final BlockEntityEntry<TrayBlockEntity> TRAY_BE = REGISTRATE
        .blockEntity("tray", TrayBlockEntity::new)
        .validBlocks(TRAY)
        .register();
}
