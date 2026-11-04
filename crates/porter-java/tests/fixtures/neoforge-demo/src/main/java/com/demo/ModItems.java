package com.demo;

import net.minecraft.resources.ResourceLocation;
import net.minecraft.world.item.Item;
import net.neoforged.neoforge.registries.DeferredItem;

// Registrations through helper methods, including a helper that calls a helper.
public class ModItems {
    public static final DeferredItem<Item> GEAR = simple("gear");
    public static final DeferredItem<Item> AXLE = tagged("axle", "c:rods");
    // The id is not a literal at the call site: counted, without an id.
    public static final DeferredItem<Item> PLATE = fromId(ResourceLocation.parse("demo:plate"));

    static DeferredItem<Item> fromId(ResourceLocation id) {
        return Demo.ITEMS.registerSimpleItem(id.getPath());
    }

    // A literal that happens to equal a parameter's name is still a literal id.
    static DeferredItem<Item> named(String name) {
        return Demo.ITEMS.registerSimpleItem("name");
    }

    static DeferredItem<Item> simple(String name) {
        return Demo.ITEMS.registerSimpleItem(name);
    }

    static DeferredItem<Item> tagged(String name, String tag) {
        return simple(name);
    }
}
