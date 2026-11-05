package net.grind;

import net.neoforged.neoforge.common.ModConfigSpec;

public final class Config {
    private static final ModConfigSpec.Builder BUILDER = new ModConfigSpec.Builder();

    static {
        BUILDER.push("crusher");
    }

    public static final ModConfigSpec.BooleanValue SOUNDS = BUILDER
        .comment("Whether crushers make a sound when they finish an item")
        .define("sounds", true);
    public static final ModConfigSpec.IntValue TIME = BUILDER
        .comment("Ticks to crush an item when its recipe names no time")
        .defineInRange("defaultTime", 100, 10, 1200);
    public static final ModConfigSpec.EnumValue<Noise> NOISE = BUILDER
        .defineEnum("noise", Noise.QUIET);

    static {
        BUILDER.pop();
    }

    public static final ModConfigSpec.DoubleValue XP = BUILDER
        .comment("Experience for each crushed item")
        .defineInRange("xp", 0.1D, 0.0D, 10.0D);

    public static final ModConfigSpec SPEC = BUILDER.build();

    public enum Noise { SILENT, QUIET, LOUD }
}
