package com.demo.mixin.client;

import com.demo.SpeedHolder;
import net.minecraft.client.gui.Gui;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Unique;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.ModifyArg;

@Mixin(Gui.class)
public class GuiMixin implements SpeedHolder {
    @Unique
    private float demo$speed;

    @ModifyArg(method = "renderHotbar",
        at = @At(value = "INVOKE", target = "Lnet/minecraft/client/gui/GuiGraphics;blit()V"), index = 1)
    private int demo$shift(int x) {
        return x + 2;
    }

    public float demo$getSpeed() {
        return demo$speed;
    }
}
