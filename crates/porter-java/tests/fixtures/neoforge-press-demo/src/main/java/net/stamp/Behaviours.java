package net.stamp;

import net.minecraft.core.BlockPos;
import net.minecraft.world.level.Level;

public class Behaviours {
    /** The behaviour of that type the block entity at `pos` has, or null. */
    public static ProcessBehaviour get(Level level, BlockPos pos, Object type) {
        if (level.getBlockEntity(pos) instanceof PressBlockEntity press) {
            return press.stamping;
        }
        return null;
    }
}
