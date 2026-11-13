package net.dial;

import java.util.function.Consumer;
import net.minecraft.nbt.CompoundTag;

/** A number in a box on a block. */
public class ScrollValueBehaviour extends BlockEntityBehaviour implements ValueSettingsBehaviour {
    ValueBoxTransform slotPositioning;
    int min = 0;
    protected int max = 1;
    public int value;
    public String label;
    Consumer<Integer> callback;

    public ScrollValueBehaviour(String label, SmartBlockEntity be, ValueBoxTransform slot) {
        super(be);
        this.label = label;
        slotPositioning = slot;
        callback = i -> {
        };
    }

    @Override
    public void write(CompoundTag nbt) {
        nbt.putInt("ScrollValue", value);
    }

    @Override
    public void read(CompoundTag nbt) {
        value = nbt.getInt("ScrollValue");
    }

    public ScrollValueBehaviour between(int min, int max) {
        this.min = min;
        this.max = max;
        return this;
    }

    public ScrollValueBehaviour withCallback(Consumer<Integer> valueCallback) {
        callback = valueCallback;
        return this;
    }

    public void setValue(int value) {
        value = Math.max(min, Math.min(max, value));
        if (value == this.value)
            return;
        this.value = value;
        callback.accept(value);
    }

    @Override
    public ValueBoxTransform getSlotPositioning() {
        return slotPositioning;
    }

    public ValueSettingsBoard createBoard() {
        return new ValueSettingsBoard(label, max, 10);
    }

    @Override
    public void setValueSettings(ValueSettings valueSetting) {
        setValue(valueSetting.value());
    }

    @Override
    public ValueSettings getValueSettings() {
        return new ValueSettings(0, value);
    }

    public record ValueSettingsBoard(String title, int maxValue, int milestoneInterval) {
    }
}
