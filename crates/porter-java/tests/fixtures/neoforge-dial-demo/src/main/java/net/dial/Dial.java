package net.dial;

import com.tterrag.registrate.Registrate;
import com.tterrag.registrate.util.entry.BlockEntityEntry;
import com.tterrag.registrate.util.entry.BlockEntry;
import net.minecraft.world.level.block.Blocks;
import net.neoforged.fml.common.Mod;
import com.simibubi.create.foundation.data.CreateRegistrate;
import com.simibubi.create.infrastructure.config.CStress;

/** A tiny Create-style mod: a motor whose speed is set in a box on it. */
@Mod(Dial.ID)
public class Dial {
    public static final String ID = "dial";
    public static final Registrate REGISTRATE = Registrate.create(ID);

    public static final BlockEntry<MotorBlock> MOTOR = REGISTRATE.block("motor", MotorBlock::new)
        .initialProperties(() -> Blocks.IRON_BLOCK)
        .lang("Dial Motor")
        .onRegister(CreateRegistrate.blockModel(() -> BracketModel::new))
        .transform(CStress.setCapacity(16384.0))
        .simpleItem()
        .register();

    public static final BlockEntityEntry<MotorBlockEntity> MOTOR_BE = REGISTRATE
        .blockEntity("motor", MotorBlockEntity::new)
        .validBlocks(MOTOR)
        .register();
}
