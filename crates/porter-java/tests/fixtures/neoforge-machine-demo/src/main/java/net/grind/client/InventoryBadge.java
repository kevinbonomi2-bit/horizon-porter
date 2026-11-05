package net.grind.client;

import net.minecraft.client.gui.screens.inventory.InventoryScreen;
import net.neoforged.neoforge.client.event.ScreenEvent;

/** Shows how many items the player's crushers have crushed, beside the inventory. */
public final class InventoryBadge {
    static long crushed;

    public static void render(ScreenEvent.Render.Post event) {
        if (event.getScreen() instanceof InventoryScreen inventory) {
            int x = inventory.getGuiLeft() + 180;
            int y = inventory.getGuiTop();
            event.getGuiGraphics().fill(x, y, x + 60, y + 14, 0x88000000);
            event.getGuiGraphics().drawString(inventory.getMinecraft().font, "Crushed " + crushed, x + 3, y + 3, 0xFFFFFF);
        }
    }
}
