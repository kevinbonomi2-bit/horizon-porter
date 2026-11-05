package net.grind.client;

import net.minecraft.client.gui.GuiGraphics;
import net.minecraft.client.gui.screens.Screen;
import net.minecraft.network.chat.Component;

/** A one-page guide to the crusher, opened with G. */
public class GuideScreen extends Screen {
    private int page;

    public GuideScreen() {
        super(Component.translatable("screen.grind.guide"));
    }

    @Override
    public void render(GuiGraphics g, int mouseX, int mouseY, float partial) {
        super.render(g, mouseX, mouseY, partial);
        int x = this.width / 2 - 100;
        int y = this.height / 2 - 60;
        g.fill(x, y, x + 200, y + 120, 0xCC000000);
        g.drawString(this.font, "Crusher: page " + (page + 1), x + 8, y + 8, 0xFFFFFF);
        g.renderItem(new net.minecraft.world.item.ItemStack(net.minecraft.world.item.Items.COBBLESTONE), x + 8, y + 24);
    }

    @Override
    public boolean mouseClicked(double mouseX, double mouseY, int button) {
        page = (page + 1) % 3;
        return true;
    }
}
