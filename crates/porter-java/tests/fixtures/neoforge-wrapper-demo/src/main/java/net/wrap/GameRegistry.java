package net.wrap;

import java.util.function.Supplier;
import net.minecraft.core.Registry;
import net.minecraft.resources.ResourceKey;
import net.neoforged.neoforge.registries.DeferredHolder;
import net.neoforged.neoforge.registries.DeferredRegister;

/** A multi-loader style wrapper: the registry is only known where it is made. */
public class GameRegistry<T> {
    protected final DeferredRegister<T> handle;

    public GameRegistry(ResourceKey<? extends Registry<T>> reg) {
        this.handle = DeferredRegister.create(reg, "wrap");
    }

    public <I extends T> DeferredHolder<T, I> register(String name, Supplier<? extends I> sup) {
        return this.handle.register(name, sup);
    }
}
