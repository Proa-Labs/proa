import React from 'react';

interface CounterProps {
  initialCount: number;
  label: string;
  disabled?: boolean;
}

export default function Counter({ initialCount, label, disabled }: CounterProps) {
  const [count, setCount] = React.useState(initialCount);

  return (
    <div>
      <span>{label}: {count}</span>
      <button disabled={disabled} onClick={() => setCount(c => c + 1)}>+</button>
    </div>
  );
}
