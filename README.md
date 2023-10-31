# ![pac ghost](assets/ghost.png "Blinku") PAC BATTLE ![pac ghost](assets/ghost.png "Blinku")

A one versus one ghost battling game to the (extra?)death.

Designed in Bevy game engine based on the following game design tutorial series: https://johanhelsing.studio/posts/extreme-bevy

### TODO

- [ ] Add a block based map to the grid
- [ ] Add collision detection to map using a simple calculation (coordinates / MAP_SIZE).floor() as index into array of blocktype at position
- [ ] auto generate the map with wave collapse or perlin noise
- [ ] update player spawn function with random locaion
- [ ] check player spawn location generation with collision to not spawn in wall