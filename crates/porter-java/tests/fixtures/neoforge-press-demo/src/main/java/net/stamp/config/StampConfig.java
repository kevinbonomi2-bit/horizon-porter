package net.stamp.config;

/** The settings of the mod, in groups, in the style of a config class with typed values. */
public class StampConfig {
    private static final StampConfig SERVER = new StampConfig();

    public final StampRecipes recipes = new StampRecipes();

    public static StampConfig server() {
        return SERVER;
    }
}
