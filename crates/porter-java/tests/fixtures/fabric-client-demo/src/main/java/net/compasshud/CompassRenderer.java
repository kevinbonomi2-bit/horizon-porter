package net.compasshud;

import net.minecraft.client.Minecraft;
import net.minecraft.client.gui.GuiGraphics;
import net.minecraft.core.Direction;

public class CompassRenderer {
    static void draw(GuiGraphics graphics, boolean shown) {
        Minecraft mc = Minecraft.getInstance();
        if (!shown || mc.player == null) {
            return;
        }
        Direction facing = mc.player.getDirection();
        graphics.drawString(mc.font, "Facing " + facing.getName(), 4, 4, 0xFFFFFF);
    }
}
