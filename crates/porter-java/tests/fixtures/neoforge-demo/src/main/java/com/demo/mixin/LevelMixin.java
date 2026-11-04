package com.demo.mixin;

import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.entity.TickingBlockEntity;
import org.spongepowered.asm.mixin.Final;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Shadow;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.Redirect;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Mixin(Level.class)
public abstract class LevelMixin {
    @Shadow @Final public boolean isClientSide;

    @Inject(method = "tickBlockEntities", at = @At("HEAD"), cancellable = true)
    private void demo$beforeTick(CallbackInfo ci) {
    }

    @Inject(method = "tickBlockEntities", at = @At("TAIL"))
    private void demo$afterTick(CallbackInfo ci) {
    }

    @Redirect(method = "tickBlockEntities",
        at = @At(value = "FIELD", target = "Lnet/minecraft/world/level/Level;tickingBlockEntities:Z"))
    private boolean demo$ticking(Level level) {
        return true;
    }

    @Redirect(method = "tickBlockEntities",
        at = @At(value = "INVOKE", target = "Lnet/minecraft/world/level/block/entity/TickingBlockEntity;tick()V"))
    private void demo$tickOne(TickingBlockEntity be) {
        be.tick();
    }
}
