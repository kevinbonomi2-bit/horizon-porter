package net.dial;

/** A value set on a board, in a box on a block. */
public interface ValueSettingsBehaviour {
    record ValueSettings(int row, int value) {
    }

    ValueBoxTransform getSlotPositioning();

    void setValueSettings(ValueSettings valueSetting);

    ValueSettings getValueSettings();
}
