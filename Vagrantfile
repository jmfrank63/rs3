# -*- mode: ruby -*-
# vi: set ft=ruby :

# All Vagrant configuration is done below. The "2" in Vagrant.configure
# configures the configuration version (we support older styles for
# backwards compatibility). Please don't change it unless you know what
# you're doing.
Vagrant.configure("2") do |config|
  # The most common configuration options are documented and commented below.
  # For a complete reference, please see the online documentation at
  # https://docs.vagrantup.com.

  # Every Vagrant development environment requires a box. You can search for
  # boxes at https://vagrantcloud.com/search.
  if ENV['VAGRANT_DETECTED_OS'] == 'Darwin'
    if ENV['VAGRANT_DETECTED_ARCH'] == 'arm64'
      config.vm.box = "rueian/ubuntu20-m1"
      config.vm.box_version = "0.0.1"
      config.vm.provider "parallels" do |prl|
        prl.linked_clone = false
        prl.update_guest_tools = true
      end
    else
      config.vm.box = "bento/ubuntu-20.04"
      config.vm.provider :vmware_desktop do |vmware|
        vmware.vmx["ethernet0.pcislotnumber"] = "32"
      end
    end
  else
    config.vm.box = "bento/ubuntu-20.04"
    config.vm.provider :vmware_desktop do |vmware|
      vmware.vmx["ethernet0.pcislotnumber"] = "32"
    end
  end

  # Disable automatic box update checking. If you disable this, then
  # boxes will only be checked for updates when the user runs
  # `vagrant box outdated`. This is not recommended.
  # config.vm.box_check_update = false

  # Create a forwarded port mapping which allows access to a specific port
  # within the machine from a port on the host machine. In the example below,
  # accessing "localhost:8080" will access port 80 on the guest machine.
  # NOTE: This will enable public access to the opened port
  # config.vm.network "forwarded_port", guest: 80, host: 8080

  # Create a forwarded port mapping which allows access to a specific port
  # within the machine from a port on the host machine and only allow access
  # via 127.0.0.1 to disable public access
#   config.vm.network "forwarded_port", guest: 80, host: 8080, host_ip: "127.0.0.1"
  config.vm.network "forwarded_port", guest: 80, guest_ip: "192.168.5.100", host: 8880, host_ip: "127.0.0.1"
  config.vm.network "forwarded_port", guest: 8080, guest_ip: "192.168.5.100", host: 8881, host_ip: "127.0.0.1"
  # config.vm.network "forwarded_port", guest: 8443, guest_ip: "192.168.5.100", host:443, host_ip: "127.0.0.1"

  # Create a private network, which allows host-only access to the machine
  # using a specific IP.
  # config.vm.network "private_network", ip: "192.168.33.10"

  # Create a public network, which generally matched to bridged network.
  # Bridged networks make the machine appear as another physical device on
  # your network.
  # config.vm.network "public_network"

  # Share an additional folder to the guest VM. The first argument is
  # the path on the host to the actual folder. The second argument is
  # the path on the guest to mount the folder. And the optional third
  # argument is a set of non-required options.
  # config.vm.synced_folder "../data", "/vagrant_data"
  if ENV['WSL_DISTRO_NAME'] == 'WLinux'
    config.vm.synced_folder "/mnt/c/Users/jmfra/Projects/Rust/rs3/docker_rs3/config", "/vagrant/docker_rs3/config"
    config.vm.synced_folder "/mnt/c/Users/jmfra/Projects/Rust/rs3", "/vagrant"
  else
    config.vm.synced_folder "docker_rs3/config", "/vagrant/docker_rs3/config"
  end

  # Provider-specific configuration so you can fine-tune various
  # backing providers for Vagrant. These expose provider-specific options.
  # Example for VirtualBox:
  #
  # config.vm.provider "virtualbox" do |vb|
  #   # Display the VirtualBox GUI when booting the machine
  #   vb.gui = true
  #
  #   # Customize the amount of memory on the VM:
  #   vb.memory = "1024"
  # end
  #
  # View the documentation for the provider you are using for more
  # information on available options.

  # Enable provisioning with a shell script. Additional provisioners such as
  # Ansible, Chef, Docker, Puppet and Salt are also available. Please see the
  # documentation for more information about their specific syntax and use.
  # config.vm.provision "shell", inline: <<-SHELL
  #   apt-get update
  #   apt-get install -y apache2
  # SHELL
  config.vm.provision "ansible" do |ansible|
    ansible.playbook = "docker_rs3/playbook_dev.yml"
  end
#     config.vm.network "forwarded_port", guest: 8080, guest_ip: "192.168.5.100", host: 8080, host_ip: "127.0.0.1"
#     config.vm.network "forwarded_port", guest: 8081, guest_ip: "192.168.5.100", host: 8081, host_ip: "127.0.0.1"
end
