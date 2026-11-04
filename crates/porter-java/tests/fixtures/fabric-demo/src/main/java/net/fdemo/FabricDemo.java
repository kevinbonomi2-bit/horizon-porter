package net.fdemo;

import net.fabricmc.api.ModInitializer;
import net.minecraft.core.Registry;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.resources.Identifier;
import net.minecraft.world.item.Item;

public class FabricDemo implements ModInitializer {
    public static final Item GEM = Registry.register(BuiltInRegistries.ITEM,
        Identifier.fromNamespaceAndPath("fdemo", "gem"), new Item(new Item.Properties()));

    @Override
    public void onInitialize() {
    }
}
