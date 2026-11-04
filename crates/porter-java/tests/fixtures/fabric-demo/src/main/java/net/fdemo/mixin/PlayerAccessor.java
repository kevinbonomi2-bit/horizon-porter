package net.fdemo.mixin;

import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.gen.Accessor;
import org.spongepowered.asm.mixin.gen.Invoker;

@Mixin(targets = "net/minecraft/world/entity/player/Player")
public interface PlayerAccessor {
    @Accessor("experienceLevel")
    int fdemo$getLevel();

    @Invoker("closeContainer")
    void fdemo$close();
}
