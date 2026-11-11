package net.stamp.config;

/** The settings for recipes. */
public class StampRecipes {
    public final ConfigBool bulk = b(false, "bulk", "Stamp a whole stack at a time.");
    public final ConfigInt reach = i(2, "reach", "How far under itself a press looks.");

    private static ConfigBool b(boolean value, String name, String comment) {
        return new ConfigBool(value);
    }

    private static ConfigInt i(int value, String name, String comment) {
        return new ConfigInt(value);
    }

    public record ConfigBool(boolean value) {
        public Boolean get() {
            return value;
        }
    }

    public record ConfigInt(int value) {
        public Integer get() {
            return value;
        }
    }
}
