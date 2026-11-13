package net.dial;

/** A speed: the row picks the way it turns. */
public class KineticScrollValueBehaviour extends ScrollValueBehaviour {
    public KineticScrollValueBehaviour(String label, SmartBlockEntity be, ValueBoxTransform slot) {
        super(label, be, slot);
    }

    @Override
    public ValueSettingsBoard createBoard() {
        return new ValueSettingsBoard(label, 256, 32);
    }

    @Override
    public void setValueSettings(ValueSettings valueSetting) {
        int value = Math.max(1, valueSetting.value());
        setValue(valueSetting.row() == 0 ? -value : value);
    }

    @Override
    public ValueSettings getValueSettings() {
        return new ValueSettings(value < 0 ? 0 : 1, Math.abs(value));
    }
}
